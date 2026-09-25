"""Long-lived BGE-M3 embedding worker for semantic search.

Started once by `EmbeddingWorker` (embeddings/worker.rs) with argv
[cache_dir, model_id] and kept alive so the model loads only once per app
session. Speaks JSON lines over stdin/stdout, one request at a time:

    {"id": 1, "op": "embed", "texts": ["...", ...]}
        -> {"id": 1, "vectors": [[1024 floats], ...]}   (L2-normalized)
    {"id": 2, "op": "correct", "words": ["tranactiosn", ...]}
        -> {"id": 2, "corrections": {"tranactiosn": "transactions"}}
    {"id": 3, "op": "ping"} -> {"id": 3, "ok": true}

Any failure is reported as {"id": ..., "error": "..."} and the worker keeps
running. Nothing but protocol lines is ever written to stdout; library
output is redirected to stderr. Everything runs on-device.
"""

import contextlib
import json
import os
import sys

os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
os.environ.setdefault("TRANSFORMERS_VERBOSITY", "error")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")

BATCH_SIZE = 16
MAX_TOKENS = 512

_stdout = sys.stdout.buffer


def respond(message):
    # Raw UTF-8 bytes: Python's text-mode stdout on Windows follows the
    # legacy codepage, which would corrupt non-ASCII text.
    _stdout.write(json.dumps(message).encode("utf-8") + b"\n")
    _stdout.flush()


class Embedder:
    def __init__(self, model_id):
        self.model_id = model_id
        self.model = None
        self.tokenizer = None
        self.torch = None

    def _load(self):
        with contextlib.redirect_stdout(sys.stderr):
            import torch
            from transformers import AutoModel, AutoTokenizer

            self.torch = torch
            self.tokenizer = AutoTokenizer.from_pretrained(self.model_id)
            self.model = AutoModel.from_pretrained(self.model_id)
            self.model.eval()

    def embed(self, texts):
        if self.model is None:
            self._load()

        torch = self.torch
        vectors = []
        for start in range(0, len(texts), BATCH_SIZE):
            batch = [text if text.strip() else " " for text in texts[start:start + BATCH_SIZE]]
            encoded = self.tokenizer(
                batch,
                padding=True,
                truncation=True,
                max_length=MAX_TOKENS,
                return_tensors="pt",
            )
            with torch.inference_mode():
                output = self.model(**encoded)
            # BGE's dense embedding is the normalized [CLS] hidden state.
            cls = torch.nn.functional.normalize(output.last_hidden_state[:, 0], p=2, dim=1)
            vectors.extend([[round(value, 6) for value in row] for row in cls.tolist()])
        return vectors


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
    cache_dir, model_id = sys.argv[1], sys.argv[2]
    # HF_HOME is the single cache location for weights, shared with Media Tagging.
    os.environ["HF_HOME"] = cache_dir

    embedder = Embedder(model_id)
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
                respond({"id": request_id, "vectors": embedder.embed(request.get("texts", []))})
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
