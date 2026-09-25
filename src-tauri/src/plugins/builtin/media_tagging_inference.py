"""Batched local image captioning for the Media Tagging built-in plugin.

Invoked as a subprocess of the bundled Python runtime (see
`run_media_captioning_batch` in plugins.rs). Reads a single JSON object from
stdin: {"modelId": str, "cacheDir": str, "paths": [str, ...]}. Loads the
captioning model once (downloading and caching it under `cacheDir` on first
use, entirely on-device — no evidence data is ever sent anywhere), then
emits exactly one JSON array on the final line of stdout, one entry per
input path in order: {"path": str, "caption": str} on success, or
{"path": str, "error": str} if that specific image could not be captioned.
A per-image failure never aborts the batch; only a failure to load the
model itself does.

Runs the model directly via PyTorch (`transformers`), not ONNX Runtime:
`optimum`'s ONNX exporter does not natively support the BLIP architecture
("custom or unsupported architecture" at export time, confirmed against a
live run), and CPU-only PyTorch ends up an unavoidable dependency of the
onnxruntime export path anyway — so exporting to ONNX would add complexity
without actually removing the torch dependency it was meant to avoid.
"""

import json
import os
import sys

os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
os.environ.setdefault("TRANSFORMERS_VERBOSITY", "error")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")


def emit(results):
    # Write raw UTF-8 bytes directly rather than using print()/sys.stdout:
    # on Windows, Python's default text-mode stdout encoding follows the
    # legacy system codepage, not UTF-8, which can corrupt non-ASCII
    # characters (e.g. a case folder name). The Rust side reads only the
    # last line of stdout as JSON, so any incidental library logging on
    # stdout before this point is tolerated.
    sys.stdout.buffer.write(json.dumps(results).encode("utf-8"))
    sys.stdout.buffer.write(b"\n")
    sys.stdout.flush()


def main():
    # Read raw bytes and decode as UTF-8 explicitly (utf-8-sig also strips
    # an optional leading byte-order mark) rather than sys.stdin.read(),
    # for the same legacy-codepage reason as emit() above.
    request = json.loads(sys.stdin.buffer.read().decode("utf-8-sig"))
    model_id = request["modelId"]
    cache_dir = request["cacheDir"]
    paths = request["paths"]

    # HF_HOME is the single source of truth for where huggingface_hub /
    # transformers cache downloaded weights; setting it here (rather than
    # also passing cache_dir= to from_pretrained) keeps everything under
    # one directory instead of two different cache layouts.
    os.environ["HF_HOME"] = cache_dir

    try:
        import torch
        from PIL import Image
        from transformers import BlipForConditionalGeneration, BlipProcessor
    except Exception as error:  # noqa: BLE001 - reported per file below
        emit([{"path": path, "error": f"Media tagging dependencies unavailable: {error}"} for path in paths])
        return

    try:
        processor = BlipProcessor.from_pretrained(model_id)
        model = BlipForConditionalGeneration.from_pretrained(model_id)
        model.eval()
    except Exception as error:  # noqa: BLE001 - a model load failure fails the whole batch
        print(json.dumps({"error": f"Failed to load media tagging model '{model_id}': {error}"}), file=sys.stderr)
        sys.exit(1)

    results = []

    for path in paths:
        try:
            image = Image.open(path).convert("RGB")
            inputs = processor(images=image, return_tensors="pt")
            with torch.no_grad():
                generated_ids = model.generate(**inputs, max_new_tokens=30)
            caption = processor.batch_decode(generated_ids, skip_special_tokens=True)[0].strip()
            results.append({"path": path, "caption": caption})
        except Exception as error:  # noqa: BLE001 - reported per file, batch continues
            results.append({"path": path, "error": str(error)})

    emit(results)


if __name__ == "__main__":
    main()
