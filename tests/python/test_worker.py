"""Exercise the actual stdin/stdout protocol in isolated worker processes."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import hashlib
import importlib.util
import io
import zipfile
from unittest.mock import patch

WORKER = Path(__file__).resolve().parents[2] / "src-tauri/src/scripting/worker/plugin.py"


class WorkerTests(unittest.TestCase):
    def run_plugin(self, script, *, protocol=1):
        with tempfile.TemporaryDirectory(prefix="cultivator-worker-") as directory:
            root = Path(directory)
            evidence = root / "evidence café.txt"
            evidence.write_text("hello evidence\n", encoding="utf-8")
            (root / "plugin.py").write_text(script, encoding="utf-8")
            task = {
                "plugin_id": "fixture", "file_path": str(evidence),
                "datasource_id": "source-1", "case_id": "case-1",
                "target_file": {"path": str(evidence), "name": evidence.name,
                                "extension": "txt", "size": evidence.stat().st_size},
            }
            request = {
                "protocol_version": protocol,
                "plugin": {"id": "fixture", "name": "Fixture", "entry": "plugin.py",
                           "function": "run", "target": "ios", "mode": "each_file"},
                "plugin_directory": str(root), "case_database_path": str(root / "case.db"),
                "case_folder_path": str(root),
                "datasource": {"id": "source-1", "case_id": "case-1", "name": "Evidence",
                               "paths": [str(evidence)]},
                "tasks": [task], "scanned_files": 1, "options": {"limit": 10},
            }
            return subprocess.run(
                [sys.executable, "-I", "-B", str(WORKER)],
                input=json.dumps(request).encode("utf-8"), capture_output=True, timeout=15,
            )

    def test_context_api_logs_and_stdout_do_not_corrupt_json(self):
        result = self.run_plugin('''
import cultivator_api as api
print("plugin import diagnostic")
def run(context):
    assert context["plugin"]["target"] == "ios"
    assert context["plugin"]["matched_files"] == [context["file"]["path"]]
    assert context["task"]["case_id"] == "case-1"
    assert context["options"]["limit"] == 10
    assert "hello evidence" in api.read_text(context["file"]["path"])
    print("plugin execution diagnostic")
    api.log("info", "record created")
    api.progress(1, 1, "Done")
    return api.create_artifact("note", "café", text="example")
''')
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        output = json.loads(result.stdout)
        self.assertEqual(output["scannedFiles"], 1)
        self.assertEqual(output["matchedFiles"], 1)
        self.assertEqual(output["records"][0]["label"], "café")
        self.assertEqual(output["logs"][0]["message"], "record created")
        self.assertIn(b"plugin import diagnostic", result.stderr)
        self.assertIn(b"__CULTIVATOR_PROGRESS__", result.stderr)

    def test_exception_is_reported_and_next_worker_recovers(self):
        broken = self.run_plugin('def run(context):\n    raise RuntimeError("fixture crash")\n')
        self.assertNotEqual(broken.returncode, 0)
        self.assertIn(b"fixture crash", broken.stderr)
        self.assertEqual(broken.stdout, b"")
        recovered = self.run_plugin('def run(context):\n    return None\n')
        self.assertEqual(recovered.returncode, 0, recovered.stderr.decode())
        self.assertEqual(json.loads(recovered.stdout)["records"], [])

    def test_table_artifacts_and_search_remain_compatible(self):
        result = self.run_plugin('''
import cultivator_api as api
def run(context):
    matches = api.search("hello")
    assert len(matches) == 1
    table = api.create_table_artifact("Results", "Custom", ["Name", "Name"])
    api.add_table_row(table, {"name": "first", "name_2": "second"})
    api.add_artifact(table, file_path=context["file"]["path"])
''')
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        record = json.loads(result.stdout)["records"][0]
        self.assertEqual(record["resultKind"], "custom_table")
        self.assertEqual(record["payload"]["table"]["rows"][0]["name_2"], "second")

    def test_protocol_version_is_checked(self):
        result = self.run_plugin('def run(context):\n    return None\n', protocol=999)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"Unsupported Cultivator worker protocol", result.stderr)


class ModelDownloadTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = WORKER.with_name("model_download.py")
        spec = importlib.util.spec_from_file_location("model_download", path)
        cls.worker = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.worker)

    def file(self, data, path="weights.bin"):
        return {"path": path, "url": "https://models.example/weights", "size": len(data), "sha256": hashlib.sha256(data).hexdigest()}

    def response(self, data, status=200, headers=None):
        response = io.BytesIO(data)
        response.status = status
        response.headers = headers or {}
        return response

    def test_resumes_partial_download_and_reuses_verified_models_offline(self):
        data = b"test model bytes"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "weights.bin.part").write_bytes(data[:5])
            with patch.object(self.worker.urllib.request, "urlopen", return_value=self.response(data[5:], 206, {"Content-Range": f"bytes 5-{len(data)-1}/{len(data)}"})) as network:
                self.worker.download_file(root, self.file(data), lambda *args: None)
                self.assertEqual(network.call_args.args[0].get_header("Range"), "bytes=5-")
            self.assertEqual((root / "weights.bin").read_bytes(), data)
            with patch.object(self.worker.urllib.request, "urlopen", side_effect=AssertionError("Unexpected network")):
                self.worker.download_file(root, self.file(data), lambda *args: None)

    def test_ignored_range_restarts_and_checksum_failure_does_not_publish(self):
        data = b"test model bytes"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "weights.bin.part").write_bytes(data[:5])
            with patch.object(self.worker.urllib.request, "urlopen", return_value=self.response(data)):
                self.worker.download_file(root, self.file(data), lambda *args: None)
            self.assertEqual((root / "weights.bin").read_bytes(), data)
            (root / "weights.bin").write_bytes(b"previous file")
            with patch.object(self.worker.urllib.request, "urlopen", return_value=self.response(b"x" * len(data))):
                with self.assertRaisesRegex(ValueError, "SHA-256"):
                    self.worker.download_file(root, self.file(data), lambda *args: None)
            self.assertEqual((root / "weights.bin").read_bytes(), b"previous file")
            self.assertFalse((root / "weights.bin.part").exists())

    def test_dependencies_archive_and_receipts_require_complete_verified_files(self):
        data = b"checkpoint bytes"
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("head.pth", data)
            archive.writestr("../unexpected", b"never extract")
        zipped = buffer.getvalue()
        file = self.file(data, "head.pth")
        file.update(archiveMember="head.pth", downloadSize=len(zipped), downloadSha256=hashlib.sha256(zipped).hexdigest())
        catalog = [
            {"id": "backbone", "dependencies": [], "files": [self.file(b"backbone")]},
            {"id": "head", "dependencies": ["backbone"], "files": [file]},
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            progress = []
            with patch.object(self.worker.urllib.request, "urlopen", side_effect=[self.response(b"backbone"), self.response(zipped)]):
                self.worker.download(root, catalog, "head", progress.append)
            self.assertEqual((root / "head/head.pth").read_bytes(), data)
            self.assertFalse((root / "unexpected").exists())
            receipt = json.loads((root / "head/.ready.json").read_text())
            self.assertEqual(receipt["files"]["head.pth"]["sha256"], file["sha256"])
            self.assertEqual(progress[-1]["downloaded"], progress[-1]["total"])
            with patch.object(self.worker.urllib.request, "urlopen", side_effect=AssertionError("Unexpected network")):
                self.worker.download(root, catalog, "head", lambda value: None)
            with self.assertRaises(ValueError):
                self.worker.destination(root, "../outside")


class FaceDetectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("face_detection", WORKER.with_name("face_detection.py"))
        cls.worker = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.worker)

    def test_boxes_are_normalized_clipped_and_ignore_invalid_detections(self):
        boxes = self.worker.face_boxes([
            [20, 10, 40, 30], [-10, 40, 30, 30], [90, 0, 20, 20],
            [0, 0, -1, 20], [float("nan"), 0, 10, 10], [200, 0, 10, 10],
        ], 100, 50)
        self.assertEqual(len(boxes), 3)
        self.assertEqual(boxes[0], {"x": 0.2, "y": 0.2, "width": 0.4, "height": 0.6})
        self.assertEqual(boxes[1], {"x": 0, "y": 0.8, "width": 0.2, "height": 0.2})
        self.assertEqual(boxes[2], {"x": 0.9, "y": 0, "width": 0.1, "height": 0.4})
        self.assertEqual(self.worker.face_boxes(None, 100, 50), [])


class ClipClassificationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = WORKER.with_name("clip_classification.py")
        spec = importlib.util.spec_from_file_location("clip_classification", path)
        cls.worker = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.worker)
        cls.taxonomy = json.loads((WORKER.parents[4] / "src/features/media/model/clipClassifications.json").read_text())

    def test_taxonomy_and_prompts_cover_all_fifteen_categories(self):
        self.assertEqual([category["id"] for category in self.taxonomy], list(range(1, 16)))
        prompts = self.worker.prompts(self.taxonomy)
        self.assertEqual(len(prompts), 3 * sum(len(category["examples"]) for category in self.taxonomy))
        self.assertTrue(any("magazines" in prompt and "weapons" in prompt for prompt in prompts))
        self.assertTrue(any("animal injuries" in prompt for prompt in prompts))

    def test_multiple_matches_keep_scores_without_forcing_unmatched_categories(self):
        taxonomy = [{"id": 1, "examples": ["handguns", "rifles"]}, {"id": 7, "examples": ["faces"]}, {"id": 14, "examples": ["dogs"]}]
        matches = self.worker.select_matches([0.2, 0.34, 0.31, 0.1], taxonomy, 0.26)
        self.assertEqual([match["category"] for match in matches], [1, 7])
        self.assertEqual(matches[0]["matchedLabel"], "rifles")
        self.assertAlmostEqual(matches[0]["score"], 0.34)
        self.assertEqual(self.worker.select_matches([0.1] * 4, taxonomy, 0.26), [])
        self.assertEqual(self.worker.select_matches([0.2, 0.34, 0.31, 0.1], taxonomy, 0.35), [])


if __name__ == "__main__":
    unittest.main()
