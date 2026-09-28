"""Long-lived BGE-M3 embedding worker for semantic search.

Started once by `EmbeddingWorker` (embeddings/worker.rs) with argv
[cache_dir, model_id, revision] and kept alive so the model loads only once per app
session. Speaks JSON lines over stdin/stdout, one request at a time:

    {"id": 1, "op": "embed", "texts": ["...", ...]}
        -> {"id": 1, "device": "...", "count": n, "vectors": "<base64>"}
           (n L2-normalized float32 vectors, little-endian, back to back;
           binary because formatting millions of floats as JSON text cost
           more than computing them)
    {"id": 2, "op": "correct", "words": ["tranactiosn", ...]}
        -> {"id": 2, "corrections": {"tranactiosn": "transactions"}}
    {"id": 3, "op": "load"} -> {"id": 3, "device": "..."}
    {"id": 4, "op": "ping"} -> {"id": 4, "ok": true}

Runs on an NVIDIA GPU in half precision when available (see
ai/python/device.py, which is prepended to this script), else the CPU.

Any failure is reported as {"id": ..., "error": "..."} and the worker keeps
running. Nothing but protocol lines is ever written to stdout; library
output is redirected to stderr. Everything runs on-device.
"""

import base64
import contextlib
import json
import os
import sys

os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
os.environ.setdefault("TRANSFORMERS_VERBOSITY", "error")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")

CPU_BATCH_SIZE = 16
GPU_BATCH_SIZE = 128
MAX_TOKENS = 512

_stdout = sys.stdout.buffer


def respond(message):
    # Raw UTF-8 bytes: Python's text-mode stdout on Windows follows the
    # legacy codepage, which would corrupt non-ASCII text.
    _stdout.write(json.dumps(message).encode("utf-8") + b"\n")
    _stdout.flush()


class Embedder:
    def __init__(self, model_id, revision):
        self.model_id = model_id
        self.revision = revision
        self.model = None
        self.tokenizer = None
        self.torch = None
        self.device = "cpu"
        self.device_name = "CPU"

    def load(self):
        if self.model is not None:
            return

        with contextlib.redirect_stdout(sys.stderr):
            import torch
            from transformers import AutoModel, AutoTokenizer

            self.torch = torch
            self.device, dtype, self.device_name = pick_device(torch, cpu_bfloat16=True)
            self.tokenizer = AutoTokenizer.from_pretrained(self.model_id, revision=self.revision)
            model = AutoModel.from_pretrained(self.model_id, revision=self.revision)
            self.model = model.to(device=self.device, dtype=dtype).eval()

    def embed(self, texts):
        self.load()

        torch = self.torch
        batch_size = GPU_BATCH_SIZE if self.device == "cuda" else CPU_BATCH_SIZE
        # Batching texts of similar length wastes far less work on padding.
        order = sorted(range(len(texts)), key=lambda index: len(texts[index]))
        vectors = None
        for start in range(0, len(order), batch_size):
            indexes = order[start:start + batch_size]
            batch = [texts[index] if texts[index].strip() else " " for index in indexes]
            try:
                rows = self.encode(batch)
            except torch.cuda.OutOfMemoryError:
                # A small GPU can't fit this batch of long texts; go one at a time.
                torch.cuda.empty_cache()
                rows = torch.cat([self.encode([text]) for text in batch])
            if vectors is None:
                vectors = torch.empty((len(texts), rows.shape[1]), dtype=torch.float32)
            vectors[indexes] = rows
        return vectors if vectors is not None else torch.empty((0, 0))

    def encode(self, batch):
        torch = self.torch
        encoded = self.tokenizer(
            batch,
            padding=True,
            truncation=True,
            max_length=MAX_TOKENS,
            return_tensors="pt",
        ).to(self.device)
        with torch.inference_mode():
            output = self.model(**encoded)
        # BGE's dense embedding is the normalized [CLS] hidden state.
        cls = torch.nn.functional.normalize(output.last_hidden_state[:, 0].float(), p=2, dim=1)
        return cls.cpu()


class Speller:
    def __init__(self):
        self.checker = None

    def correct(self, words):
        if self.checker is None:
            with contextlib.redirect_stdout(sys.stderr):
                from spellchecker import SpellChecker

                self.checker = SpellChecker(distance=2)

        lowered = [word.lower() for word in words if word.isalpha()]
        corrections = {}
        for word in self.checker.unknown(lowered):
            suggestion = self.checker.correction(word)
            if suggestion and suggestion != word:
                corrections[word] = suggestion
        return corrections


def main():
    cache_dir, model_id, revision = sys.argv[1], sys.argv[2], sys.argv[3]
    # HF_HOME is the single cache location for weights, shared with Media Tagging.
    os.environ["HF_HOME"] = cache_dir

    embedder = Embedder(model_id, revision)
    model = f"{model_id}@{revision[:7]}"
    speller = Speller()

    for raw in sys.stdin.buffer:
        line = raw.decode("utf-8-sig").strip()
        if not line:
            continue

        request_id = None
        try:
            request = json.loads(line)
            request_id = request.get("id")
            op = request.get("op")
            if op == "embed":
                vectors = embedder.embed(request.get("texts", []))
                respond({
                    "id": request_id,
                    "device": embedder.device_name,
                    "model": model,
                    "count": vectors.shape[0],
                    "vectors": base64.b64encode(vectors.numpy().astype("<f4").tobytes()).decode("ascii"),
                })
            elif op == "load":
                embedder.load()
                respond({"id": request_id, "device": embedder.device_name, "model": model})
            elif op == "correct":
                respond({"id": request_id, "corrections": speller.correct(request.get("words", []))})
            elif op == "ping":
                respond({"id": request_id, "ok": True})
            else:
                respond({"id": request_id, "error": f"Unknown op: {op}"})
        except Exception as error:  # noqa: BLE001 - reported to the caller, worker stays up
            respond({"id": request_id, "error": f"{type(error).__name__}: {error}"})


if __name__ == "__main__":
    main()
