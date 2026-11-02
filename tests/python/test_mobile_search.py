"""Compare indexed glob searches with LEAPP's original full-scan semantics."""
import fnmatch
import glob
import importlib.util
import os
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

WORKER = Path(__file__).resolve().parents[2] / "src-tauri/src/scripting/worker/mobile_artifacts.py"
spec = importlib.util.spec_from_file_location("mobile_search_worker", WORKER)
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class SearchIndexTests(unittest.TestCase):
    def setUp(self):
        paths = [f"device/app-{n}/Library/{name}" for n in range(400)
                 for name in ("History.db", "History.db-wal", "note.txt", "Mixed.PLIST", "noextension")]
        paths += ["device/folder[1]", "device/folder[1]/nested", "device/folder[1]/nested/a.db",
                  "device/folder[1]/a.db", "device/folder10/b.db", "device/AddressBook/sub/other.db"]
        self.entries = [{"path": path, "kind": "directory" if path.endswith(("[1]", "nested")) else "file"}
                        for path in reversed(paths)]
        self.lookup = worker.EvidencePathIndex(self.entries)

    def test_candidates_preserve_all_glob_matches_and_first_hit_order(self):
        patterns = ["*History.db", "*/History.db", "*/*.db", "*/*.PLIST", "*/*.plist",
                    "*/app-?/*/*", "*/app-[12]/*/note.txt", "*", "*noextension", "*/missing.db",
                    "*.db*", "*/folder[[]1]/*", "*/Library/*.[dt][bx]*", "*nested*",
                    "*/AddressBook*.db", glob.escape("root/device/folder[1]/a.db")]
        for pattern in patterns:
            matcher = fnmatch._compile_pattern(os.path.normcase(pattern))
            expected = [item for item in self.entries if matcher(os.path.normcase("root/" + item["path"]))]
            with self.subTest(pattern=pattern):
                self.assertEqual(self.lookup.match(pattern), expected)
                self.assertEqual(self.lookup.match(pattern, first=True), expected[:1])

    def test_directory_descendants_preserve_inventory_order_and_boundaries(self):
        directory = "device/folder[1]"
        prefix = worker.path_key(directory) + os.sep
        expected = [item["path"] for item in self.entries if worker.path_key(item["path"]).startswith(prefix)]
        self.assertEqual(list(self.lookup.descendants(directory)), expected)
        self.assertEqual(list(self.lookup.descendants(directory, files_only=True)),
                         [path for path in expected if not path.endswith("nested")])

    def test_absent_literal_filename_avoids_examining_entire_inventory(self):
        self.assertEqual(self.lookup.match("*/absent.sqlite"), [])
        self.assertEqual(self.lookup.stats["candidates"], 0)
        self.assertEqual(self.lookup.stats["fullScanCandidates"], len(self.entries))

    def test_index_reader_shares_metadata_strings_without_changing_records(self):
        records = [{"path": "a", "kind": "file", "provenance": {"source": "same extraction label"}},
                   {"path": "b", "kind": "file", "provenance": {"source": "same extraction label"}}]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "index.ndjson"
            path.write_text("\n".join(map(json.dumps, records)) + "\n\n", encoding="utf-8")
            loaded = worker.load_evidence_index(path)
        self.assertEqual(loaded, records)
        self.assertIs(loaded[0]["provenance"]["source"], loaded[1]["provenance"]["source"])
        self.assertIs(next(iter(loaded[0])), next(iter(loaded[1])))

    def test_staging_failure_restores_complete_upstream_path_list(self):
        class FailingSeeker:
            def search(self, *args):
                raise OSError("Staging failed")

        seeker = worker.evidence_seeker(FailingSeeker, self.entries)()
        seeker.searched, seeker.copied = {}, {}
        original = [entry["path"] for entry in self.entries]
        seeker._all_files = original
        with mock.patch.object(worker, "materialize"), self.assertRaises(OSError):
            seeker.search("*/History.db")
        self.assertIs(seeker._all_files, original)


if __name__ == "__main__":
    unittest.main()
