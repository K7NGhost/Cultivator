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


class FaceAnalyzer:
    """Each CPU worker owns its detector; OpenCV DNN nets are not thread safe."""
    def __init__(self, directory):
        import cv2
        import threading
        cv2.setNumThreads(1)
        self.model = model_path(directory)
        self.local = threading.local()

    def detect(self, image):
        import cv2
        import numpy as np
        detector = getattr(self.local, "detector", None)
        if detector is None:
            detector = cv2.FaceDetectorYN.create(self.model, "", (320, 320), 0.8, 0.3, 5000)
            self.local.detector = detector
        image.thumbnail((1280, 1280))
        frame = cv2.cvtColor(np.asarray(image.convert("RGB")), cv2.COLOR_RGB2BGR)
        detector.setInputSize((frame.shape[1], frame.shape[0]))
        _, faces = detector.detect(frame)
        boxes = face_boxes(faces, frame.shape[1], frame.shape[0])
        return {"count": len(boxes), "boxes": boxes}


def main():
    import os
    from concurrent.futures import ThreadPoolExecutor
    from PIL import Image, ImageOps
    import pillow_heif

    pillow_heif.register_heif_opener(depth_images=False, aux_images=False, decode_threads=1)
    analyzer = FaceAnalyzer(sys.argv[1])
    workers = min(16, max(1, int(sys.argv[2]))) if len(sys.argv) > 2 else min(8, os.cpu_count() or 1)
    def analyze(path):
        try:
            with Image.open(path) as source:
                source.draft("RGB", (1280, 1280))
                return analyzer.detect(ImageOps.exif_transpose(source))
        except Exception as error:
            return {"error": str(error)}
    emit({"ready": True, "workers": workers})
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for line in sys.stdin:
            request = json.loads(line)
            if "paths" in request:
                emit({"results": list(pool.map(analyze, request["paths"]))})
            else:
                emit(analyze(request["path"]))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        emit({"error": str(error)})
        sys.exit(1)
