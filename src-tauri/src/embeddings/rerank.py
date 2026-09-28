"""Long-lived precision check for semantic search (Qwen3-Reranker-0.6B).

Started once by `RerankWorker` (embeddings/rerank.rs) with argv
[cache_dir, model_id, revision]. BGE-M3 finds candidates by comparing
vectors, which is fast but approximate; this small language model reads the
question and each candidate together and answers "yes" or "no" to whether
they are related. Speaks JSON lines over stdin/stdout, one request at a time:

    {"id": 1, "op": "check", "query": "...", "texts": ["...", ...]}
        -> {"id": 1, "device": "...", "model": "...", "logits": [-2.1, -8.4, ...]}
           (for each text, the log-odds of "yes": logit(yes) - logit(no))
    {"id": 2, "op": "fetch"} -> {"id": 2, "ok": true}
           (downloads the model without loading it)
    {"id": 3, "op": "load"} -> {"id": 3, "device": "...", "model": "..."}
    {"id": 4, "op": "ping"} -> {"id": 4, "ok": true}

The prompt is the model's documented format. Its start (instruction and
query) is identical for every text, so it is computed once per request and
reused, which makes a check about 3x faster on a CPU with the same results.

Runs on an NVIDIA GPU in half precision when available (see
ai/python/device.py, which is prepended to this script), else the CPU.
Weights are read from safetensors only, never pickle.

Any failure is reported as {"id": ..., "error": "..."} and the worker keeps
running. Nothing but protocol lines is ever written to stdout; library
output is redirected to stderr. Everything runs on-device.
"""

import contextlib
import copy
import json
import os
import sys

os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
os.environ.setdefault("TRANSFORMERS_VERBOSITY", "error")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")

CPU_BATCH_SIZE = 16
GPU_BATCH_SIZE = 64
MAX_TOKENS = 512
PROMPT_START = (
    "<|im_start|>system\nJudge whether the Document meets the requirements based on the Query "
    "and the Instruct provided. Note that the answer can only be \"yes\" or \"no\".<|im_end|>\n"
    "<|im_start|>user\n"
)
PROMPT_END = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"
# Chosen on the demo and 100,000-message test phones: of the wordings tried,
# this one best separated real matches (including indirect ones, like a
# transfer for "money") from unrelated filler.
INSTRUCTION = "Judge whether the message relates to the query topic, directly or indirectly"
# config.json last: Cultivator treats a model as downloaded once its
# config.json is on disk, so an interrupted download is retried.
MODEL_FILES = [
    "model.safetensors",
    "merges.txt",
    "vocab.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "generation_config.json",
    "config.json",
]

_stdout = sys.stdout.buffer


def respond(message):
    # Raw UTF-8 bytes: Python's text-mode stdout on Windows follows the
    # legacy codepage, which would corrupt non-ASCII text.
    _stdout.write(json.dumps(message).encode("utf-8") + b"\n")
    _stdout.flush()


class Reranker:
    def __init__(self, model_id, revision):
        self.model_id = model_id
        self.revision = revision
        self.model = None
        self.tokenizer = None
        self.torch = None
        self.device = "cpu"
        self.device_name = "CPU"

    def fetch(self):
        with contextlib.redirect_stdout(sys.stderr):
            from huggingface_hub import hf_hub_download

            for name in MODEL_FILES:
                hf_hub_download(self.model_id, name, revision=self.revision)

    def load(self):
        if self.model is not None:
            return

        with contextlib.redirect_stdout(sys.stderr):
            import torch
            from transformers import AutoModelForCausalLM, AutoTokenizer

            self.torch = torch
            self.device, dtype, self.device_name = pick_device(torch, cpu_bfloat16=True)
            self.tokenizer = AutoTokenizer.from_pretrained(self.model_id, revision=self.revision)
            model = AutoModelForCausalLM.from_pretrained(
                self.model_id, revision=self.revision, use_safetensors=True
            )
            self.model = model.to(device=self.device, dtype=dtype).eval()
            # Only the "yes" and "no" rows of the output layer are ever needed;
            # projecting onto the whole vocabulary would cost more than the check.
            answers = self.tokenizer.convert_tokens_to_ids(["yes", "no"])
            self.answer_weights = self.model.lm_head.weight[answers].detach().float()
            self.end_ids = self.tokenizer(PROMPT_END, add_special_tokens=False).input_ids
        # The first run on a CPU is ~3 s slower while kernels are prepared;
        # pay that here, while the examiner is still typing.
        self.check("warm up", ["warm up"])

    def check(self, query, texts):
        self.load()

        torch = self.torch
        # Split where the tokenizer would split anyway (before the space that
        # starts the document), so the tokens match the unsplit prompt.
        start = self.tokenizer(
            f"{PROMPT_START}<Instruct>: {INSTRUCTION}\n<Query>: {query}\n<Document>:",
            add_special_tokens=False,
            return_tensors="pt",
        ).input_ids.to(self.device)
        with torch.inference_mode():
            start_cache = self.model(input_ids=start, use_cache=True).past_key_values

        batch_size = GPU_BATCH_SIZE if self.device == "cuda" else CPU_BATCH_SIZE
        # Batching texts of similar length wastes far less work on padding.
        order = sorted(range(len(texts)), key=lambda index: len(texts[index]))
        logits = [0.0] * len(texts)
        for first in range(0, len(order), batch_size):
            indexes = order[first:first + batch_size]
            batch = [texts[index] for index in indexes]
            try:
                results = self.score(start, start_cache, batch)
            except torch.cuda.OutOfMemoryError:
                # A small GPU can't fit this batch of long texts; go one at a time.
                torch.cuda.empty_cache()
                results = [value for text in batch for value in self.score(start, start_cache, [text])]
            for index, value in zip(indexes, results):
                logits[index] = value
        return logits

    def score(self, start, start_cache, batch):
        torch = self.torch
        documents = self.tokenizer(
            [f" {text}" for text in batch],
            add_special_tokens=False,
            truncation=True,
            max_length=MAX_TOKENS,
        ).input_ids
        encoded = self.tokenizer.pad(
            {"input_ids": [document + self.end_ids for document in documents]},
            padding=True,
            padding_side="right",
            return_tensors="pt",
        ).to(self.device)
        cache = copy.deepcopy(start_cache)
        cache.batch_repeat_interleave(len(batch))
        attention_mask = torch.cat(
            [
                torch.ones((len(batch), start.shape[1]), dtype=encoded.attention_mask.dtype, device=self.device),
                encoded.attention_mask,
            ],
            dim=1,
        )
        with torch.inference_mode():
            hidden = self.model.model(
                input_ids=encoded.input_ids,
                attention_mask=attention_mask,
                past_key_values=cache,
            ).last_hidden_state
            # Right padding: each prompt's answer is predicted at its last real token.
            last = encoded.attention_mask.sum(dim=1) - 1
            answer = hidden[torch.arange(len(batch), device=self.device), last].float() @ self.answer_weights.T
        return (answer[:, 0] - answer[:, 1]).cpu().tolist()


def main():
    cache_dir, model_id, revision = sys.argv[1], sys.argv[2], sys.argv[3]
    # HF_HOME is the single cache location for weights, shared with the other models.
    os.environ["HF_HOME"] = cache_dir

    reranker = Reranker(model_id, revision)
    model = f"{model_id}@{revision[:7]}"

    for raw in sys.stdin.buffer:
        line = raw.decode("utf-8-sig").strip()
        if not line:
            continue

        request_id = None
        try:
            request = json.loads(line)
            request_id = request.get("id")
            op = request.get("op")
            if op == "check":
                logits = reranker.check(request.get("query", ""), request.get("texts", []))
                respond({"id": request_id, "device": reranker.device_name, "model": model, "logits": logits})
            elif op == "fetch":
                reranker.fetch()
                respond({"id": request_id, "ok": True})
            elif op == "load":
                reranker.load()
                respond({"id": request_id, "device": reranker.device_name, "model": model})
            elif op == "ping":
                respond({"id": request_id, "ok": True})
            else:
                respond({"id": request_id, "error": f"Unknown op: {op}"})
        except Exception as error:  # noqa: BLE001 - reported to the caller, worker stays up
            respond({"id": request_id, "error": f"{type(error).__name__}: {error}"})


if __name__ == "__main__":
    main()
