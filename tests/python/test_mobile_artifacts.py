"""Catalog completeness and real end-to-end mobile artifact protocol tests.

Set CULTIVATOR_IOS_TEST_PYTHON / CULTIVATOR_ANDROID_TEST_PYTHON /
CULTIVATOR_VEHICLE_TEST_PYTHON to interpreters
with their bundled requirements installed to enable the integration tests.
"""
import ast
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import queue
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock
import zipfile

ROOT = Path(__file__).resolve().parents[2]
BUNDLES = ROOT / "src-tauri/resources/mobile"
WORKER = ROOT / "src-tauri/src/scripting/worker/mobile_artifacts.py"


def interpreter(platform):
    variable = {"ios": "CULTIVATOR_IOS_TEST_PYTHON", "android": "CULTIVATOR_ANDROID_TEST_PYTHON",
                "vehicle": "CULTIVATOR_VEHICLE_TEST_PYTHON"}[platform]
    candidate = os.environ.get(variable)
    return Path(candidate) if candidate else None


def invoke_worker(platform, request, *, provide=None, timeout=180):
    python = interpreter(platform)
    process = subprocess.Popen([str(python), "-I", "-B", str(WORKER), str(BUNDLES / platform)],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, encoding="utf-8", errors="strict")
    events, materialized = [], []
    messages = queue.Queue()
    def read_messages():
        for line in process.stdout:
            messages.put(line)
        messages.put(None)
    reader = threading.Thread(target=read_messages, daemon=True)
    reader.start()
    try:
        process.stdin.write(json.dumps(request) + "\n")
        process.stdin.flush()
        while True:
            line = messages.get(timeout=timeout)
            if line is None:
                break
            event = json.loads(line)
            events.append(event)
            if event["type"] == "materialize":
                materialized.extend(event["paths"])
                if provide:
                    provide(event["paths"])
                process.stdin.write('{"ok":true}\n')
                process.stdin.flush()
        status = process.wait(timeout=5)
        errors = process.stderr.read()
        if status:
            raise AssertionError(f"Worker failed ({status}): {events[-4:]}\n{errors}")
        return events, materialized
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=10)
        for stream in (process.stdin, process.stdout, process.stderr):
            stream.close()
        reader.join(timeout=5)


def create_index(root, output, *, only=None):
    entries = []
    for path in sorted(root.rglob("*")):
        if only and path not in only and not path.is_dir():
            continue
        relative = path.relative_to(root).as_posix()
        entries.append({"path": str(path), "relative": relative,
                        "kind": "directory" if path.is_dir() else "file",
                        "provenance": {"source": "fixture.E01", "evidencePath": "/" + relative,
                                       "metadata": {"size": path.stat().st_size, "times": {"modified": {
                                           "seconds": int(path.stat().st_mtime), "nanos": 0, "basis": "utc"}}}}})
    output.write_text("".join(json.dumps(item) + "\n" for item in entries), encoding="utf-8")
    return entries


class MobileCatalogTests(unittest.TestCase):
    def test_managed_python_policy_excludes_external_parsers_and_dependencies(self):
        sys.path.insert(0, str(ROOT / "scripts/mobile"))
        try:
            from runtime_policy import EXCLUDED_MODULES, include_policy_file, filter_requirements
            for platform in ("ios", "android", "vehicle"):
                bundle = BUNDLES / platform
                catalog = json.loads((bundle / "catalog.json").read_text(encoding="utf-8"))
                self.assertEqual(catalog["packaging"]["dependencyPolicy"], "managed-python-only-v1")
                self.assertEqual(catalog["packaging"]["excludedModules"], EXCLUDED_MODULES[platform])
                for relative in EXCLUDED_MODULES[platform]:
                    self.assertFalse((bundle / relative).exists(), relative)
                for relative in catalog["files"]:
                    self.assertTrue(include_policy_file(relative, platform), relative)
                requirements = (bundle / "requirements.txt").read_text(encoding="utf-8")
                self.assertEqual(filter_requirements(requirements, platform), requirements)
                for folder in ("artifacts", "alternate_artifacts"):
                    for path in (bundle / "scripts" / folder).glob("*.py"):
                        tree = ast.parse(path.read_text(encoding="utf-8-sig"))
                        for node in ast.walk(tree):
                            imports = [n.name for n in node.names] if isinstance(node, ast.Import) else [node.module or ""] if isinstance(node, ast.ImportFrom) else []
                            for name in imports:
                                self.assertNotIn(name.split(".")[0], {"ctypes", "subprocess", "sqlcipher3", "pysqlcipher3", "mister_skinnylegs", "ccl_chromium_reader"}, str(path))
        finally:
            sys.path.pop(0)

    def test_vendoring_does_not_restore_external_dependencies(self):
        sys.path.insert(0, str(ROOT / "scripts/mobile"))
        try:
            import vendor_sources
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / "upstream"
                artifacts = source / "scripts/artifacts"
                artifacts.mkdir(parents=True)
                definition = "__artifacts_v2__ = {'fixture': {'name': 'Fixture', 'paths': ('*/fixture.db',)}}\n"
                (artifacts / "safe.py").write_text(definition, encoding="utf-8")
                (artifacts / "Threema.py").write_text(definition.replace("fixture", "excluded"), encoding="utf-8")
                wheels = source / "whl_files"
                wheels.mkdir()
                (wheels / "sqlcipher3-0.6.2-cp315-cp315-win_amd64.whl").write_bytes(b"excluded")
                requirements = "sqlcipher3==0.6.2\nmister_skinnylegs @ git+https://example.invalid/repo\nprotobuf==5.29.6\n"
                (source / "requirements.txt").write_text(requirements, encoding="utf-8")
                with mock.patch.object(vendor_sources, "DESTINATION", root / "bundles"), mock.patch.object(vendor_sources.subprocess, "check_output", side_effect=["fixture-revision\n", ""]):
                    vendor_sources.vendor(source, "android")
                bundle = root / "bundles/android"
                catalog = json.loads((bundle / "catalog.json").read_text(encoding="utf-8"))
                self.assertEqual(catalog["counts"]["v2"], 1)
                self.assertEqual(catalog["upstreamCounts"]["v2"], 2)
                self.assertFalse((bundle / "scripts/artifacts/Threema.py").exists())
                self.assertFalse(list(bundle.rglob("*.whl")))
                self.assertNotIn("git+", (bundle / "requirements.txt").read_text(encoding="utf-8"))
                self.assertNotIn("sqlcipher3", (bundle / "requirements.txt").read_text(encoding="utf-8"))
                self.assertEqual((source / "requirements.txt").read_text(encoding="utf-8"), requirements)
        finally:
            sys.path.pop(0)

    def test_headless_bundles_exclude_report_assets(self):
        for platform in ("ios", "android", "vehicle"):
            bundle = BUNDLES / platform
            catalog = json.loads((bundle / "catalog.json").read_text(encoding="utf-8"))
            self.assertEqual(catalog["packaging"]["profile"], "cultivator-headless")
            for directory in ("assets", "scripts/_elements"):
                self.assertFalse((bundle / directory).exists(), str(bundle / directory))
                self.assertFalse(any(path.startswith(directory + "/") for path in catalog["files"]))

    def test_every_runtime_file_matches_the_snapshot(self):
        for platform in ("ios", "android", "vehicle"):
            with self.subTest(platform=platform):
                bundle = BUNDLES / platform
                catalog = json.loads((bundle / "catalog.json").read_text(encoding="utf-8"))
                for name, digest in catalog["files"].items():
                    self.assertEqual(hashlib.sha256((bundle / name).read_bytes()).hexdigest(), digest, name)
                self.assertFalse(any(bundle.rglob("*.pyc")))

    def test_all_primary_and_alternate_definitions_are_catalogued(self):
        for platform in ("ios", "android", "vehicle"):
            bundle = BUNDLES / platform
            catalog = json.loads((bundle / "catalog.json").read_text(encoding="utf-8"))
            catalogued = {module["path"]: module for module in catalog["modules"]}
            discovered = {}
            for folder in ("artifacts", "alternate_artifacts"):
                for path in (bundle / "scripts" / folder).rglob("*.py"):
                    if path.name == "__init__.py":
                        continue
                    keys = set()
                    tree = ast.parse(path.read_text(encoding="utf-8-sig"))
                    for node in tree.body:
                        if isinstance(node, ast.Assign) and isinstance(node.value, ast.Dict):
                            if any(isinstance(target, ast.Name) and target.id in ("__artifacts_v2__", "__artifacts__") for target in node.targets):
                                keys.update(ast.literal_eval(key) for key in node.value.keys)
                    discovered[path.relative_to(bundle).as_posix()] = keys
            self.assertEqual(set(discovered), set(catalogued))
            for path, keys in discovered.items():
                self.assertEqual(keys, {item["key"] for item in catalogued[path]["artifacts"]}, path)
            expected = catalog["counts"]["v1"] + catalog["counts"]["v2"]
            self.assertEqual(sum(map(len, discovered.values())), expected)
            self.assertGreater(expected, 0)


class MobileIntegrationTests(unittest.TestCase):
    def require_python(self, platform):
        python = interpreter(platform)
        if not python or not python.is_file():
            self.skipTest(f"Set the {platform} test interpreter to run bundled parser integration tests")

    def test_load_every_ios_parser(self):
        self.require_python("ios")
        events, _ = invoke_worker("ios", {"action": "validate"})
        self.assertEqual(events[-1]["type"], "done")
        self.assertTrue(events[-1]["catalogVerified"])
        self.assertGreater(next(e["artifacts"] for e in events if e["type"] == "catalog"), 1000)

    def test_load_every_android_parser(self):
        self.require_python("android")
        events, _ = invoke_worker("android", {"action": "validate"})
        self.assertEqual(events[-1]["type"], "done")
        self.assertTrue(events[-1]["catalogVerified"])

    def test_load_every_vehicle_parser(self):
        self.require_python("vehicle")
        events, _ = invoke_worker("vehicle", {"action": "validate"})
        self.assertEqual(next(e["artifacts"] for e in events if e["type"] == "catalog"), 211)
        self.assertTrue(events[-1]["catalogVerified"])

    def exercise(self, platform, build_fixture, expected_text, *, deferred=False):
        self.require_python(platform)
        with tempfile.TemporaryDirectory(prefix="cultivator-mobile-") as directory:
            case = Path(directory)
            root = case / "input"
            root.mkdir()
            build_fixture(root)
            untouched = root / "unrelated-secret.bin"
            untouched.write_bytes(b"not an artifact")
            original = {str(p): p.read_bytes() for p in root.rglob("*") if p.is_file()}
            index = case / "index.ndjson"
            create_index(root, index)
            if deferred:
                for path in original:
                    Path(path).unlink()
            def provide(paths):
                for path in paths:
                    self.assertIn(path, original)
                    if not Path(path).exists():
                        Path(path).write_bytes(original[path])
            events, materialized = invoke_worker(platform, {"roots": [{"path": str(root), "indexPath": str(index)}],
                                                           "outputRoot": str(case / "output"), "options": {"timezone": "UTC"}}, provide=provide)
            artifacts = [event for event in events if event["type"] == "artifact"]
            self.assertTrue(artifacts, events[-8:])
            self.assertIn(expected_text, json.dumps(artifacts))
            self.assertNotIn(str(untouched), materialized)
            self.assertTrue(any(e["type"] == "progress" for e in events))
            self.assertEqual(events[-1]["type"], "done")
            self.assertEqual(events[-1]["failures"], [], [e for e in events if e["type"] == "log" and "Traceback" in e["message"]])
            for path, data in original.items():
                if deferred and path not in materialized:
                    self.assertFalse(Path(path).exists())
                else:
                    self.assertEqual(Path(path).read_bytes(), data)
            coverage = json.loads((case / "output/coverage.json").read_text())
            self.assertGreater(len(coverage[0]["modules"]), 0)
            if platform == "vehicle":
                self.assertEqual(len(coverage[0]["modules"]), 211)
            tables = [a["payload"]["table"] for a in artifacts]
            # Independently compare each imported table batch to its source LAVA rows.
            database = sqlite3.connect(case / "output/source-1/_lava_artifacts.db")
            try:
                for event in artifacts:
                    payload = event["payload"]
                    name = payload["parser"]["artifact"]["tablename"]
                    query = 'SELECT * FROM "' + name.replace('"', '""') + '" ORDER BY rowid LIMIT ? OFFSET ?'
                    batch = payload["table"]["rows"]
                    values = database.execute(query, (len(batch), payload["parser"]["rowOffset"])).fetchall()
                    self.assertEqual([tuple(row.values()) for row in batch], values)
                    for column in payload["table"]["columns"]:
                        self.assertEqual(column["label"], payload["parser"]["artifact"]["column_map"][column["key"]])
            finally:
                database.close()
            return events, materialized

    def test_ios_system_version_and_source_preservation(self):
        def fixture(root):
            path = root / "System/Library/CoreServices/SystemVersion.plist"
            path.parent.mkdir(parents=True)
            path.write_bytes(plistlib.dumps({"ProductName": "iPhone OS", "ProductVersion": "18.3.2",
                                             "ProductBuildVersion": "22D82"}))
        events, _ = self.exercise("ios", fixture, "22D82")
        version = next(e for e in events if e["type"] == "artifact" and e["label"] == "System Version plist")
        self.assertTrue(version["payload"]["sources"])
        self.assertEqual(version["payload"]["provenance"]["source"], "fixture.E01")

    def test_android_build_properties_and_source_preservation(self):
        def fixture(root):
            path = root / "system/build.prop"
            path.parent.mkdir(parents=True)
            path.write_text("ro.build.version.release=15\nro.product.model=Cultivator Fixture\n", encoding="utf-8")
        self.exercise("android", fixture, "Cultivator Fixture")

    def test_vehicle_ford_and_connected_devices(self):
        def fixture(root):
            (root / "bluetooth_v1.ddb").write_text("<KeyValuePairs>\n<Key>device_name</Key>\n<Value>Ford Fixture</Value>\n</KeyValuePairs>\n", encoding="utf-8")
            with contextlib.closing(sqlite3.connect(root / "devices.db")) as database:
                database.execute("CREATE TABLE device_master (last_connected_time INTEGER, name TEXT, uuid TEXT, favorite INTEGER, visibility INTEGER)")
                database.execute("INSERT INTO device_master VALUES (1700000000,'Fixture Phone','fixture-device',1,1)")
                database.commit()
        events, _ = self.exercise("vehicle", fixture, "Ford Fixture", deferred=True)
        device = next(e for e in events if e["type"] == "artifact" and e["label"] == "Connected Devices In devices.db")
        self.assertIn("Fixture Phone", json.dumps(device))
        self.assertEqual(device["payload"]["provenance"]["source"], "fixture.E01")

    def test_vehicle_nested_iva_export(self):
        self.require_python("vehicle")
        with tempfile.TemporaryDirectory(prefix="cultivator-iva-") as directory:
            root = Path(directory)
            path = root / "fixture.iVa"
            def archive(entries):
                buffer = io.BytesIO()
                with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED) as output:
                    for name, contents in entries.items():
                        output.writestr(name, contents)
                return buffer.getvalue()
            source = archive({"FileSystems/vehicle/bluetooth_v1.ddb": "<KeyValuePairs>\n<Key>device_name</Key>\n<Value>Nested Ford Fixture</Value>\n</KeyValuePairs>\n"})
            inner = archive({"DCASourceFilesUpload.zip": source})
            vehicle = {"Collection": {"SelectedVehicle": {"VehicleDisplay": "Fixture Vehicle", "Vin": "TEST-VIN"},
                                      "CollectionDate": "2026-10-24", "Acquisitions": [{"EcuName": "Infotainment", "AcqType": "Logical", "NumContacts": 2}]}}
            contents = archive({"export.zip": inner, "Vehicle.json": json.dumps(vehicle)})
            # The container initially exists only in the index, like an Inodex working path.
            index = root / "index.ndjson"
            entry = {"path": str(path), "relative": path.name, "kind": "file",
                     "provenance": {"source": "vehicle.E01", "evidencePath": "/fixture.iVa"}}
            index.write_text(json.dumps(entry) + "\n", encoding="utf-8")
            def provide(paths):
                self.assertEqual(paths, [str(path)])
                path.write_bytes(contents)
            events, materialized = invoke_worker("vehicle", {"roots": [{"path": str(path), "indexPath": str(index), "inputType": "iva"}],
                                                             "outputRoot": str(root / "output"), "options": {}}, provide=provide)
            artifacts = [event for event in events if event["type"] == "artifact"]
            self.assertIn("Nested Ford Fixture", json.dumps(artifacts))
            self.assertTrue(any(event["label"] == "Berla iVe Export Record" for event in artifacts))
            self.assertEqual(events[-1]["failures"], [])
            self.assertEqual(materialized, [str(path)])
            self.assertEqual(path.read_bytes(), contents)
            for event in artifacts:
                self.assertEqual(event["payload"]["provenance"]["source"], "vehicle.E01")
                self.assertEqual(event["filePath"], str(path))

    def test_vehicle_invalid_iva_fails_explicitly(self):
        self.require_python("vehicle")
        with tempfile.TemporaryDirectory(prefix="cultivator-bad-iva-") as directory:
            root = Path(directory)
            path = root / "bad.iva"
            path.write_bytes(b"not an IVA archive")
            index = root / "index.ndjson"
            index.write_text(json.dumps({"path": str(path), "relative": path.name, "kind": "file", "provenance": {}}) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(AssertionError, "could not process"):
                invoke_worker("vehicle", {"roots": [{"path": str(path), "indexPath": str(index), "inputType": "iva"}],
                                          "outputRoot": str(root / "output"), "options": {}})

    def test_deferred_files_and_inventory_metadata(self):
        def fixture(root):
            path = root / "system/build.prop"
            path.parent.mkdir(parents=True)
            path.write_text("ro.product.model=Deferred Fixture\n", encoding="utf-8")
        events, _ = self.exercise("android", fixture, "Deferred Fixture", deferred=True)
        inventory = [event for event in events if event["type"] == "artifact" and event["label"] == "App File Inventory"]
        self.assertTrue(inventory)
        self.assertIn("unrelated-secret.bin", json.dumps(inventory))

    def test_ios_itunes_hashed_file_lookup(self):
        def fixture(root):
            (root / "Info.plist").write_bytes(plistlib.dumps({"Product Version": "18.3.2", "Device Name": "Backup Fixture"}))
            (root / "Manifest.plist").write_bytes(plistlib.dumps({"IsEncrypted": False}))
            relative = "System/Library/CoreServices/SystemVersion.plist"
            file_id = hashlib.sha1(("HomeDomain-" + relative).encode()).hexdigest()
            payload = plistlib.dumps({"ProductVersion": "18.3.2", "ProductBuildVersion": "22D82"})
            path = root / file_id[:2] / file_id
            path.parent.mkdir()
            path.write_bytes(payload)
            with contextlib.closing(sqlite3.connect(root / "Manifest.db")) as database:
                database.execute("CREATE TABLE Files (fileID TEXT, domain TEXT, relativePath TEXT, flags INTEGER, file BLOB)")
                database.execute("INSERT INTO Files VALUES (?,?,?,?,?)", (file_id, "HomeDomain", relative, 1,
                    plistlib.dumps({"Birth": 1700000000, "LastModified": 1700000000, "Size": len(payload)})))
                database.commit()
        events, materialized = self.exercise("ios", fixture, "Backup Fixture")
        self.assertTrue(any(e["type"] == "artifact" and "iTunes Backup" in e["label"] for e in events))
        self.assertTrue(any(len(Path(path).name) == 40 for path in materialized))

    def test_android_sqlite_wal_and_table_batches(self):
        self.require_python("android")
        def fixture(root):
            path = root / "data/data/com.android.chrome/app_chrome/Default/History"
            path.parent.mkdir(parents=True)
            # Snapshot while the connection is open, so committed rows remain
            # in WAL rather than being checkpointed into the main database.
            with tempfile.TemporaryDirectory(prefix="cultivator-wal-source-") as directory:
                source = Path(directory) / "History"
                database = sqlite3.connect(source)
                try:
                    database.execute("PRAGMA journal_mode=WAL")
                    database.execute("PRAGMA wal_autocheckpoint=0")
                    database.execute("CREATE TABLE urls (id INTEGER, url TEXT, title TEXT, visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER, hidden INTEGER)")
                    database.execute("CREATE TABLE visits (id INTEGER, url INTEGER, visit_time INTEGER, from_visit INTEGER, transition INTEGER)")
                    database.execute("CREATE TABLE keyword_search_terms (keyword_id INTEGER, url_id INTEGER, term TEXT, normalized_term TEXT)")
                    for number in range(530):
                        database.execute("INSERT INTO urls VALUES (?,?,?,?,?,?,?)", (number + 1, f"https://example.test/wal/{number}", "WAL Fixture", 1, 1, 13300000000000000 + number, 0))
                    database.commit()
                    for file in source.parent.glob("History*"):
                        shutil.copyfile(file, path.parent / file.name)
                finally:
                    database.close()
        events, materialized = self.exercise("android", fixture, "https://example.test/wal/529")
        history = [e for e in events if e["type"] == "artifact" and e["label"] == "Web History"]
        self.assertEqual(sum(len(e["payload"]["table"]["rows"]) for e in history), 530)
        self.assertEqual(len(history), 3)
        self.assertTrue(any(path.endswith("History-wal") for path in materialized))


if __name__ == "__main__":
    unittest.main()
