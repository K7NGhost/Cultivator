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
    torch.set_num_threads(4)
    torch.set_num_interop_threads(1)
    pillow_heif.register_heif_opener(depth_images=False, aux_images=False, decode_threads=1)
    device = "cuda" if torch.cuda.is_available() else "cpu"
    model, _, preprocess = open_clip.create_model_and_transforms(
        architecture, pretrained=str(checkpoint), device=device,
    )
    model.eval()
    tokenizer = open_clip.get_tokenizer(architecture)
    with torch.inference_mode():
        # Encode once in bounded batches, including on CPU.
        tokens = tokenizer(prompts(taxonomy))
        features = torch.cat([model.encode_text(batch.to(device), normalize=True)
                              for batch in tokens.split(32)])
    emit({"ready": True, "device": device})
    for line in sys.stdin:
        try:
            request = json.loads(line)
            with Image.open(request["path"]) as source:
                source.draft("RGB", (1024, 1024))
                image = ImageOps.exif_transpose(source).convert("RGB")
                image.thumbnail((1024, 1024))
                tensor = preprocess(image).unsqueeze(0).to(device)
            with torch.inference_mode():
                embedding = model.encode_image(tensor, normalize=True)
                # Keep raw wording plus clarified and close-up/portrait prompts.
                scores = (embedding @ features.T)[0].reshape(-1, 3).max(dim=1).values.float().cpu().tolist()
            emit({"classifications": select_matches(scores, taxonomy, threshold)})
        except Exception as error:
            emit({"error": str(error)})


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        emit({"error": str(error)})
        sys.exit(1)
