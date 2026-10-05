"""Exercise the actual stdin/stdout protocol in isolated worker processes."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
