"""Exercise gallery decoding under a low-memory system's allocation budget."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

try:
    from PIL import Image
except ImportError:
    Image = None

WORKER = Path(__file__).resolve().parents[2] / "src-tauri/src/scripting/worker/media_preview.py"


@unittest.skipIf(Image is None, "Requires media-preview Pillow dependencies")
class MediaMemoryTests(unittest.TestCase):
    def test_phone_jpeg_is_downsampled_with_a_128_mib_budget(self):
        with tempfile.TemporaryDirectory(prefix="cultivator-memory-") as directory:
            source = Path(directory) / "phone.jpg"
            target = Path(directory) / "preview.jpg"
            Image.new("RGB", (4032, 3024), "green").save(source)
            result = subprocess.run(
                [sys.executable, "-I", "-B", str(WORKER), str(source), str(target), "256", str(128 * 1024 * 1024)],
                capture_output=True, timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            with Image.open(target) as preview:
                self.assertEqual(preview.size, (256, 192))

    def test_oversized_png_is_rejected_without_writing_a_preview(self):
        with tempfile.TemporaryDirectory(prefix="cultivator-memory-") as directory:
            source = Path(directory) / "large.png"
            target = Path(directory) / "preview.jpg"
            Image.new("RGB", (2048, 2048), "green").save(source)
            result = subprocess.run(
                [sys.executable, "-I", "-B", str(WORKER), str(source), str(target), "256", str(128 * 1024 * 1024)],
                capture_output=True, timeout=30,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"memory budget", result.stderr)
            self.assertFalse(target.exists())
