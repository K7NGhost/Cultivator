"""One YuNet model per scan; JSON lines in and out, no media leaves this process."""
import hashlib
import json
import math
from pathlib import Path
import sys

MODEL_SHA256 = "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4"


def model_path(root):
    target = Path(root) / "face_detection_yunet_2023mar.onnx"
    if not target.is_file() or hashlib.sha256(target.read_bytes()).hexdigest() != MODEL_SHA256:
        raise ValueError("Download or repair YuNet in Settings > Plugin models before running face detection")
    return str(target)


def emit(value):
    print(json.dumps(value), flush=True)


def face_boxes(faces, width, height):
    """Normalized boxes in the EXIF-oriented image, independent of thumbnail size."""
    boxes = []
    if faces is None:
        return boxes
    for face in faces:
        x, y, w, h = map(float, face[:4])
        if not all(math.isfinite(value) for value in (x, y, w, h)) or w <= 0 or h <= 0:
            continue
        left, top = max(0, min(width, x)), max(0, min(height, y))
        right, bottom = max(0, min(width, x + w)), max(0, min(height, y + h))
        if right > left and bottom > top:
            boxes.append({"x": left / width, "y": top / height,
                          "width": (right - left) / width, "height": (bottom - top) / height})
    return boxes


def main():
    import cv2
    import numpy as np
    from PIL import Image, ImageOps
    import pillow_heif

    pillow_heif.register_heif_opener(depth_images=False, aux_images=False, decode_threads=1)
    cv2.setNumThreads(2)
    detector = cv2.FaceDetectorYN.create(model_path(sys.argv[1]), "", (320, 320), 0.8, 0.3, 5000)
    emit({"ready": True})
    for line in sys.stdin:
        try:
            request = json.loads(line)
            # Decode originals, not gallery thumbnails: tiny thumbnails lose faces.
            # Bound inference resolution and handle Unicode paths and EXIF rotation.
            with Image.open(request["path"]) as source:
                source.draft("RGB", (1280, 1280))
                image = ImageOps.exif_transpose(source)
                image.thumbnail((1280, 1280))
                frame = cv2.cvtColor(np.asarray(image.convert("RGB")), cv2.COLOR_RGB2BGR)
            detector.setInputSize((frame.shape[1], frame.shape[0]))
            _, faces = detector.detect(frame)
            boxes = face_boxes(faces, frame.shape[1], frame.shape[0])
            emit({"count": len(boxes), "boxes": boxes})
        except Exception as error:
            # A corrupt/unsupported image is unknown, never a negative detection.
            emit({"error": str(error)})


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        emit({"error": str(error)})
        sys.exit(1)
