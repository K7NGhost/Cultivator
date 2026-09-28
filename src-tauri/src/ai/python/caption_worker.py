"""Long-lived BLIP captioning worker for the Media Tagging plugin.

Started once by `CaptionWorker` (ai/captioning.rs) with argv [cache_dir,
model_id, revision] and kept alive, so the model loads once per app session instead
of once per batch of photos. Speaks JSON lines over stdin/stdout, one
request at a time:

    {"id": 1, "op": "caption", "paths": ["...", ...]}
        -> {"id": 1, "device": "...", "results": [{"path", "caption"} | {"path", "error"}, ...]}
    {"id": 2, "op": "load"} -> {"id": 2, "device": "..."}
    {"id": 3, "op": "ping"} -> {"id": 3, "ok": true}

Results come back in the order of `paths`. One unreadable photo never fails
the batch; it gets its own "error" entry. Nothing but protocol lines is
written to stdout. Everything runs on-device.
"""

import contextlib
import json
import os
import sys

os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
os.environ.setdefault("TRANSFORMERS_VERBOSITY", "error")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")

GPU_BATCH_SIZE = 16
CPU_BATCH_SIZE = 4
# JPEGs are decoded straight to about this size; BLIP only looks at 384 x 384,
# and a full 12-megapixel decode costs more than captioning on a GPU.
DECODE_SIZE = (768, 768)
MAX_NEW_TOKENS = 30
# Stops greedy decoding looping on text-heavy images ("vendor - vendor - ...").
NO_REPEAT_NGRAM_SIZE = 3

_stdout = sys.stdout.buffer


def respond(message):
    # Raw UTF-8 bytes: Python's text-mode stdout on Windows follows the
    # legacy codepage, which would corrupt non-ASCII paths.
    _stdout.write(json.dumps(message).encode("utf-8") + b"\n")
    _stdout.flush()


class Captioner:
    def __init__(self, model_id, revision):
        self.model_id = model_id
        self.revision = revision
        self.model = None

    def load(self):
        if self.model is not None:
            return

        with contextlib.redirect_stdout(sys.stderr):
            import torch
            from PIL import Image, ImageOps
            from transformers import BlipForConditionalGeneration, BlipProcessor

            try:
                import pillow_heif

                pillow_heif.register_heif_opener()
            except Exception:  # noqa: BLE001 - HEIC support is optional
                pass

            self.torch = torch
            self.image_module = Image
            self.image_ops = ImageOps
            self.device, self.dtype, self.device_name = pick_device(torch)
            self.processor = BlipProcessor.from_pretrained(self.model_id, revision=self.revision)
            model = BlipForConditionalGeneration.from_pretrained(self.model_id, revision=self.revision)
            self.model = model.to(device=self.device, dtype=self.dtype).eval()

    def open_image(self, path):
        image = self.image_module.open(path)
        image.draft("RGB", DECODE_SIZE)
        # Phones store most photos sideways plus an orientation tag.
        image = self.image_ops.exif_transpose(image)
        return image.convert("RGB")

    def generate(self, images):
        inputs = self.processor(images=images, return_tensors="pt").to(self.device, self.dtype)
        with self.torch.inference_mode():
            generated = self.model.generate(
                **inputs,
                max_new_tokens=MAX_NEW_TOKENS,
                no_repeat_ngram_size=NO_REPEAT_NGRAM_SIZE,
            )
        return [text.strip() for text in self.processor.batch_decode(generated, skip_special_tokens=True)]

    def caption_chunk(self, images):
        try:
            return self.generate(images)
        except Exception:  # noqa: BLE001 - retried one by one below
            # Find the image that broke the batch; the others still get captions.
            outcomes = []
            for image in images:
                try:
                    outcomes.append(self.generate([image])[0])
                except Exception as error:  # noqa: BLE001 - reported for this file only
                    outcomes.append(error)
            return outcomes

    def caption(self, paths):
        self.load()
        results = [None] * len(paths)
        opened = []

        for index, path in enumerate(paths):
            try:
                opened.append((index, self.open_image(path)))
            except Exception as error:  # noqa: BLE001 - reported for this file only
                results[index] = {"path": path, "error": f"Could not open image: {error}"}

        batch_size = GPU_BATCH_SIZE if self.device == "cuda" else CPU_BATCH_SIZE
        for start in range(0, len(opened), batch_size):
            chunk = opened[start:start + batch_size]
            outcomes = self.caption_chunk([image for _, image in chunk])
            for (index, _), outcome in zip(chunk, outcomes):
                if isinstance(outcome, Exception):
                    results[index] = {"path": paths[index], "error": str(outcome)}
                else:
                    results[index] = {"path": paths[index], "caption": outcome}

        return results


def main():
    cache_dir, model_id, revision = sys.argv[1], sys.argv[2], sys.argv[3]
    # HF_HOME is the single cache location for weights, shared with semantic search.
    os.environ["HF_HOME"] = cache_dir
    captioner = Captioner(model_id, revision)
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
            if op == "caption":
                results = captioner.caption(request.get("paths", []))
                respond({"id": request_id, "device": captioner.device_name, "model": model, "results": results})
            elif op == "load":
                captioner.load()
                respond({"id": request_id, "device": captioner.device_name, "model": model})
            elif op == "ping":
                respond({"id": request_id, "ok": True})
            else:
                respond({"id": request_id, "error": f"Unknown op: {op}"})
        except Exception as error:  # noqa: BLE001 - reported to the caller, worker stays up
            respond({"id": request_id, "error": f"{type(error).__name__}: {error}"})


if __name__ == "__main__":
    main()
