"""Local multi-label OpenCLIP classification; one model and prompt bank per run."""
import json
import math
from pathlib import Path
import sys


def select_matches(scores, taxonomy, threshold):
    """Keep independent category matches; cosine similarity is not a probability."""
    results = []
    offset = 0
    for category in taxonomy:
        count = len(category["examples"])
        values = scores[offset:offset + count]
        best = max(range(count), key=lambda index: values[index])
        score = float(values[best])
        if math.isfinite(score) and score >= threshold:
            results.append({"category": category["id"], "score": round(score, 5), "matchedLabel": category["examples"][best]})
        offset += count
    return sorted(results, key=lambda match: (-match["score"], match["category"]))


def prompts(taxonomy):
    # Short CLIP prompts work better than appending an entire category description.
    # Preserve the original examples as labels; clarify only ambiguous terms.
    terms = {
        "handguns": "a handgun", "rifles": "a rifle", "shotguns": "a shotgun",
        "knives": "a knife", "magazines": "firearm ammunition magazines and weapons",
        "packaging": "drug packaging", "scales": "drug weighing scales",
        "single person": "a person", "groups": "a group of people",
        "faces": "a human face", "windows": "building windows",
        "dogs": "a dog", "cats": "a cat", "cars": "a car", "trucks": "a truck",
        "motorcycles": "a motorcycle", "boats": "a boat", "phones": "a phone",
        "computers": "a computer", "passports": "a passport",
    }
    result = []
    for category in taxonomy:
        for example in category["examples"]:
            term = terms.get(example.lower(), example.lower())
            result.append(f"A photo of {example.lower()}.")
            result.append(f"A photo of {term}.")
            result.append(f"A portrait of {term}." if example.lower() in ("single person", "faces") else f"A close-up photo of {term}.")
    return result


def emit(value):
    print(json.dumps(value), flush=True)


def load_nsfw_head(directory, device):
    """LAION head used by NeMo Curator: normalized 768-D ViT-L/14 inputs.

    Architecture and normalization match NVIDIA's pinned NSFWModel implementation:
    https://github.com/NVIDIA/NeMo-Curator/blob/87d4a1740d8a1e8185fa0c49c28e9ebb2d54c412/nemo_curator/models/nsfw.py
    Only tensor weights from the verified local model are deserialized.
    """
    import torch
    import torch.nn.functional as functional
    checkpoint = Path(directory) / "clip_autokeras_binary_nsfw.pth"
    weights = torch.load(checkpoint, map_location="cpu", weights_only=True)
    expected = {"norm.mean": (768,), "norm.variance": (768,)}
    for index, (inputs, outputs) in enumerate([(768, 64), (64, 512), (512, 256), (256, 1)], 1):
        expected[f"linear_{index}.weight"] = (outputs, inputs)
        expected[f"linear_{index}.bias"] = (outputs,)
    if set(weights) != set(expected) or any(tuple(weights[key].shape) != shape for key, shape in expected.items()):
        raise ValueError("Invalid NSFW classifier weights; repair the model in Settings")
    if not all(torch.isfinite(value).all() for value in weights.values()) or not (weights["norm.variance"] > 0).all():
        raise ValueError("Invalid NSFW classifier normalization")
    weights = {key: value.to(device=device, dtype=torch.float32) for key, value in weights.items()}
    def score(embeddings):
        value = functional.normalize(embeddings.float(), dim=-1)
        value = (value - weights["norm.mean"]) / weights["norm.variance"].sqrt()
        for index in range(1, 5):
            value = functional.linear(value, weights[f"linear_{index}.weight"], weights[f"linear_{index}.bias"])
            if index < 4:
                value = functional.relu(value)
        return value.sigmoid().flatten()
    return score


def main():
    import torch
    import open_clip
    from PIL import Image, ImageOps
    import pillow_heif

    directory = Path(sys.argv[1])
    architecture = sys.argv[2]
    threshold = float(sys.argv[3])
    taxonomy = json.loads(sys.argv[4])
    checkpoint = directory / "open_clip_model.safetensors"
    if not checkpoint.is_file():
        raise ValueError("Download OpenCLIP in Settings > Plugin models first")
    from concurrent.futures import ThreadPoolExecutor
    from contextlib import nullcontext
    config = json.loads(sys.argv[5]) if len(sys.argv) > 5 else {}
    classify_clip = config.get("clipCategories", True)
    workers = min(16, max(1, config.get("workers", 4)))
    torch.set_num_threads(min(4, workers))
    torch.set_num_interop_threads(1)
    pillow_heif.register_heif_opener(depth_images=False, aux_images=False, decode_threads=1)
    device = "cuda" if torch.cuda.is_available() else "cpu"
    batch_size = min(config.get("batchSize", 32), 128) if device == "cuda" else min(8, workers)
    if architecture.startswith("ViT-L"):
        batch_size = min(batch_size, 32)
    if device == "cuda":
        torch.backends.cuda.matmul.allow_tf32 = True
        torch.backends.cudnn.allow_tf32 = True
    model, _, preprocess = open_clip.create_model_and_transforms(
        architecture, pretrained=str(checkpoint), device=device if classify_clip else "cpu",
    )
    if not classify_clip:
        # NSFW-only runs need the image tower, not text encoding or category prompts.
        model = model.visual.to(device)
    model.eval()
    def precision():
        return torch.autocast("cuda", dtype=torch.float16) if device == "cuda" else nullcontext()
    if classify_clip:
        tokenizer = open_clip.get_tokenizer(architecture)
        with torch.inference_mode(), precision():
            tokens = tokenizer(prompts(taxonomy))
            features = torch.cat([model.encode_text(batch.to(device), normalize=True)
                                  for batch in tokens.split(64)])
    nsfw_head = None
    nsfw_encoder = None
    nsfw_preprocess = None
    if config.get("nsfwModel"):
        nsfw_head = load_nsfw_head(config["nsfwModel"], device)
        # ViT-B/32 has 512-D embeddings: never feed those to the 768-D NSFW head.
        if architecture != "ViT-L-14-quickgelu":
            backbone, _, nsfw_preprocess = open_clip.create_model_and_transforms(
                "ViT-L-14-quickgelu",
                pretrained=str(Path(config["nsfwBackbone"]) / "open_clip_model.safetensors"),
                device="cpu",
            )
            nsfw_encoder = backbone.visual.to(device).eval()
            del backbone
        batch_size = min(batch_size, 32)
    analyzer = None
    if config.get("faceModel"):
        namespace = {"__name__": "face_worker"}
        exec(sys.argv[6], namespace)
        analyzer = namespace["FaceAnalyzer"](config["faceModel"])

    def prepare(path):
        try:
            with Image.open(path) as source:
                edge = 1280 if analyzer else 1024
                source.draft("RGB", (edge, edge))
                image = ImageOps.exif_transpose(source).convert("RGB")
                image.thumbnail((edge, edge))
            # Face errors remain independent of successful CLIP results.
            face = None
            if analyzer:
                try:
                    face = analyzer.detect(image)
                except Exception as error:
                    face = {"error": str(error)}
            return (preprocess(image), nsfw_preprocess(image) if nsfw_preprocess else None), face, None
        except Exception as error:
            return None, {"error": str(error)} if analyzer else None, str(error)

    def encode(tensors):
        nonlocal batch_size
        try:
            tensor = torch.stack([pair[0] for pair in tensors])
            if device == "cuda":
                tensor = tensor.pin_memory()
            tensor = tensor.to(device, non_blocking=device == "cuda")
            with torch.inference_mode(), precision():
                if classify_clip:
                    embedding = model.encode_image(tensor, normalize=True)
                    scores = (embedding @ features.T).reshape(len(tensors), -1, 3).max(dim=2).values.float().cpu().tolist()
                else:
                    embedding = model(tensor)
                    scores = [None] * len(tensors)
            nsfw_scores = [None] * len(tensors)
            nsfw_error = None
            if nsfw_head:
                try:
                    with torch.inference_mode():
                        if nsfw_encoder is not None:
                            nsfw_tensor = torch.stack([pair[1] for pair in tensors])
                            if device == "cuda":
                                nsfw_tensor = nsfw_tensor.pin_memory()
                            nsfw_tensor = nsfw_tensor.to(device, non_blocking=device == "cuda")
                            with precision():
                                nsfw_embedding = nsfw_encoder(nsfw_tensor)
                        else:
                            nsfw_embedding = embedding
                        # The classifier's normalization and dense layers stay float32.
                        nsfw_scores = nsfw_head(nsfw_embedding).cpu().tolist()
                        if not all(math.isfinite(value) and 0 <= value <= 1 for value in nsfw_scores):
                            raise ValueError("Invalid NSFW classifier scores")
                except torch.cuda.OutOfMemoryError:
                    raise
                except Exception as error:
                    nsfw_error = str(error)
            return list(zip(scores, nsfw_scores, [nsfw_error] * len(tensors)))
        except torch.cuda.OutOfMemoryError:
            if len(tensors) == 1:
                raise
            # Retry smaller batches on shared/small GPUs instead of failing the run.
            batch_size = max(1, len(tensors) // 2)
        # Leave the exception scope and release failed activations before retrying.
        del tensor
        # Release any second-backbone activations before retrying smaller batches.
        if "nsfw_tensor" in locals():
            del nsfw_tensor
        if "nsfw_embedding" in locals():
            del nsfw_embedding
        if "embedding" in locals():
            del embedding
        torch.cuda.empty_cache()
        midpoint = len(tensors) // 2
        return encode(tensors[:midpoint]) + encode(tensors[midpoint:])

    emit({"ready": True, "device": torch.cuda.get_device_name(0) if device == "cuda" else "cpu",
          "batchSize": batch_size, "workers": workers, "nsfw": nsfw_head is not None, "clipCategories": classify_clip})
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for line in sys.stdin:
            request = json.loads(line)
            paths = request.get("paths", [request.get("path")])
            # Rust bounds requests to 128 images. Decode the next images while the GPU runs.
            pending = [pool.submit(prepare, path) for path in paths]
            results = []
            offset = 0
            while offset < len(pending):
                prepared = [future.result() for future in pending[offset:offset + batch_size]]
                offset += len(prepared)
                valid = [tensor for tensor, _, error in prepared if error is None]
                try:
                    scores = iter(encode(valid)) if valid else iter([])
                    for _, face, error in prepared:
                        if error:
                            result = {"error": error}
                        else:
                            similarities, nsfw_score, nsfw_error = next(scores)
                            result = {"classifications": select_matches(similarities, taxonomy, threshold)} if classify_clip else {}
                            if nsfw_head:
                                result.update({"nsfwError": nsfw_error} if nsfw_error else {"nsfwScore": nsfw_score})
                        if analyzer:
                            result["faces"] = face
                        results.append(result)
                except Exception as error:
                    results.extend({"error": str(error), **({"faces": face} if analyzer else {})} for _, face, _ in prepared)
            emit({"results": results} if "paths" in request else results[0])


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        emit({"error": str(error)})
        sys.exit(1)
