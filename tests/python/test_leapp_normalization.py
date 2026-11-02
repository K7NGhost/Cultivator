"""Lossless mapping, conversion, coverage, and real SQLite adapter tests."""
import contextlib
import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
WORKER = ROOT / "src-tauri/src/scripting/worker"


def load(name):
    spec = importlib.util.spec_from_file_location(name, WORKER / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class NormalizationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.engine = load("leapp_normalization")
        cls.adapter = load("mobile_artifacts")
        cls.rules = json.loads((WORKER / "leapp_normalization_rules.json").read_text())
        cls.schemas = json.loads((WORKER / "artifact_model_schemas.json").read_text())

    def test_reviewed_rules_match_bundled_catalogs_and_normalize_valid_rows(self):
        engine = self.engine.LeappNormalizer(self.rules, self.schemas)
        for rule in self.rules["rules"]:
            with self.subTest(rule=rule["id"]):
                catalog = json.loads((ROOT / "src-tauri/resources/mobile" / rule["platform"] / "catalog.json").read_text())
                keys = {artifact["key"] for module in catalog["modules"] for artifact in module["artifacts"]}
                self.assertIn(rule["artifactKey"], keys)
                columns = [{"key": name, "label": name} for name in rule["columns"]]
                artifact = {"artifact_key": rule["artifactKey"], "tablename": "fixture", "module": "fixture"}
                plan = engine.plan(catalog, artifact, columns)
                self.assertEqual(plan["status"], "mapped")
                row = dict.fromkeys(rule["columns"])
                for field, spec in rule["fields"].items():
                    row[spec["columns"][0]] = "2026-01-02T03:04:05+00:00" if spec["convert"] == "datetime" else 2 if spec["convert"] in ("number", "integer") else "Fixture"
                base = {"label": "Fixture", "source": {"filePath": "evidence.db"}, "parser": {"revision": catalog["revision"]}, "table": {"columns": columns}}
                normalized = engine.normalize(plan, row, base, 7, artifact)
                self.assertIsNotNone(normalized, plan["errors"])
                self.assertEqual(normalized["kind"], rule["kind"])
                self.assertEqual(normalized["raw"]["row"], row)
                self.assertEqual(normalized["parser"]["rowOffset"], 7)

    def test_conversions_do_not_guess_epochs_or_invent_timezones(self):
        convert = self.engine.convert_value
        self.assertEqual(convert("1500", "milliseconds_to_seconds"), 1.5)
        self.assertEqual(convert("0", "integer"), 0)
        self.assertEqual(convert("false", "boolean"), False)
        self.assertEqual(convert("+1-555-0100", "strings"), ["+1-555-0100"])
        self.assertEqual(convert('["a","b"]', "strings"), ["a", "b"])
        self.assertEqual(convert(0, "unix_seconds"), "1970-01-01T00:00:00+00:00")
        self.assertEqual(convert(1000, "unix_milliseconds"), "1970-01-01T00:00:01+00:00")
        self.assertEqual(convert(0, "cocoa_seconds"), "2001-01-01T00:00:00+00:00")
        self.assertEqual(convert(0, "webkit_microseconds"), "1601-01-01T00:00:00+00:00")
        self.assertEqual(convert("2026-01-02 03:04:05", "datetime"), "2026-01-02T03:04:05")
        self.assertEqual(convert(1700000000, "datetime", "datetime"), "2023-11-14T22:13:20+00:00")
        for value, kind in [(1700000000, "datetime"), ("not a date", "datetime"), ("nan", "number"), ("1.5", "integer"), ("unknown", "boolean")]:
            with self.assertRaises((ValueError, TypeError)):
                convert(value, kind)

    def test_sms_addresses_follow_direction_instead_of_misidentifying_outgoing_senders(self):
        engine = self.engine.LeappNormalizer(self.rules, self.schemas)
        rule = next(rule for rule in self.rules["rules"] if rule["artifactKey"] == "get_sms_mms")
        catalog = {"platform": "android", "revision": self.rules["sourceRevisions"]["android"], "files": {rule["module"]: rule["moduleSha256"]}}
        columns = [{"key": key, "label": key} for key in rule["columns"]]
        artifact = {"artifact_key": "get_sms_mms", "tablename": "sms"}
        plan = engine.plan(catalog, artifact, columns)
        base = {"label": "SMS", "source": {"filePath": "messages.db"}, "parser": {"revision": catalog["revision"]}, "table": {"columns": columns}}
        row = dict.fromkeys(rule["columns"])
        row.update(Address="+1-555-0100", Body="Fixture", Type="Sent")
        outgoing = engine.normalize(plan, row, base, 0, artifact)
        self.assertNotIn("sender", outgoing)
        self.assertEqual(outgoing["recipients"], ["+1-555-0100"])
        row["Type"] = "Received"
        incoming = engine.normalize(plan, row, base, 1, artifact)
        self.assertEqual(incoming["sender"], "+1-555-0100")
        self.assertNotIn("recipients", incoming)
        row["Type"] = "Unknown"
        unknown = engine.normalize(plan, row, base, 2, artifact)
        self.assertNotIn("sender", unknown)
        self.assertNotIn("recipients", unknown)

    def fixture(self, directory, rows, *, key="get_chrome", revision=None, omit_url=False, source_hash=None):
        root = Path(directory)
        rule = next(rule for rule in self.rules["rules"] if rule["artifactKey"] == "get_chrome")
        labels = [name for name in rule["columns"] if not (omit_url and name == "URL")]
        columns = [f"field_{index}" for index in range(len(labels))]
        source = root / "evidence.db"
        source.write_bytes(b"Original evidence bytes")
        with contextlib.closing(sqlite3.connect(root / "_lava_artifacts.db")) as database:
            database.execute("CREATE TABLE history (" + ",".join(columns) + ")")
            database.executemany("INSERT INTO history VALUES (" + ",".join("?" for _ in columns) + ")",
                                 [[row.get(label) for label in labels] for row in rows])
            database.commit()
        artifact = {"artifact_key": key, "tablename": "history", "name": "Web History", "module": "chrome",
                    "column_map": dict(zip(columns, labels)), "source_path": "evidence.db"}
        (root / "_lava_data.lava").write_text(json.dumps({"artifacts": {"Chromium": [artifact]}, "modules": []}))
        catalog = {"project": "ALEAPP", "platform": "android", "revision": revision or self.rules["sourceRevisions"]["android"],
                   "files": {rule["module"]: source_hash or rule["moduleSha256"]}}
        index = [{"relative": "evidence.db", "path": str(source), "kind": "file", "provenance": {"source": "fixture.E01", "evidencePath": "/evidence.db"}}]
        return root, catalog, index, source

    def run_fixture(self, directory, rows, **options):
        disabled = options.pop("disabled", False)
        root, catalog, index, source = self.fixture(directory, rows, **options)
        before = hashlib.sha256((root / "_lava_artifacts.db").read_bytes()).hexdigest()
        events = []
        normalizer = self.adapter.create_normalizer({"options": {"normalizeArtifacts": not disabled}})
        with mock.patch.object(self.adapter, "emit", side_effect=events.append):
            count, failures, _ = self.adapter.import_tables(root, root, index, catalog, 0, normalizer)
        self.assertEqual(count, len(rows))
        self.assertEqual(failures, [])
        self.assertEqual(before, hashlib.sha256((root / "_lava_artifacts.db").read_bytes()).hexdigest())
        self.assertEqual(source.read_bytes(), b"Original evidence bytes")
        return [event for event in events if event["type"] == "artifact"], json.loads((root / "normalization.json").read_text())

    def test_mixed_rows_preserve_every_original_row_and_offset_without_duplicates(self):
        rows = [{"URL": f"https://example.test/{index}", "Title": "Fixture", "Visit Count": count,
                 "Last Visit Time": "2026-01-02T03:04:05+00:00", "Typed Count": 0, "ID": index, "Hidden": 0}
                for index, count in enumerate(["bad", "1", "bad", "bad", "2"])]
        with tempfile.TemporaryDirectory() as directory:
            events, report = self.run_fixture(directory, rows)
        self.assertEqual(report["totals"], {"rows": 5, "normalizedRows": 2, "fallbackRows": 3, "invalidRows": 3})
        self.assertEqual([event["payload"]["parser"]["rowOffset"] for event in events], [0, 1, 2, 4])
        self.assertEqual([event["payload"]["kind"] for event in events], ["custom_table", "browser_history", "custom_table", "browser_history"])
        originals = []
        for event in events:
            payload = event["payload"]
            originals.extend(payload["table"]["rows"] if payload["kind"] == "custom_table" else [payload["raw"]["row"]])
            self.assertEqual(payload["provenance"]["source"], "fixture.E01")
            self.assertEqual(payload["source"]["filePath"], event["filePath"])
        self.assertEqual(len(originals), 5)
        self.assertEqual([list(row.values())[0] for row in originals], [row["Last Visit Time"] for row in rows])
        self.assertIn("field_4", report["tables"][0]["unmappedColumns"])

    def test_unknown_changed_disabled_and_schema_mismatch_outputs_stay_custom(self):
        for options, status in [({"key": "unknown"}, "unmapped"), ({"revision": "new-revision"}, "revision_mismatch"),
                                ({"omit_url": True}, "schema_mismatch"), ({"disabled": True}, "disabled"), ({"source_hash": "changed"}, "source_mismatch")]:
            with self.subTest(status=status), tempfile.TemporaryDirectory() as directory:
                events, report = self.run_fixture(directory, [{"URL": "https://example.test", "Visit Count": 1}], **options)
                self.assertEqual(events[0]["payload"]["kind"], "custom_table")
                self.assertEqual(report["tables"][0]["status"], status)
                self.assertEqual(report["totals"]["fallbackRows"], 1)

    def test_unmapped_rows_keep_the_original_256_row_batch_bound(self):
        with tempfile.TemporaryDirectory() as directory:
            events, report = self.run_fixture(directory, [{"URL": str(index)} for index in range(530)], key="unknown")
        self.assertEqual([len(event["payload"]["table"]["rows"]) for event in events], [256, 256, 18])
        self.assertEqual(report["totals"]["rows"], 530)

    def test_audit_covers_all_catalog_entries_without_enabling_candidates(self):
        path = ROOT / "scripts/mobile/audit_normalization.py"
        spec = importlib.util.spec_from_file_location("audit_normalization", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        report = module.audit()
        expected = sum(sum(len(item["artifacts"]) for item in json.loads((ROOT / "src-tauri/resources/mobile" / platform / "catalog.json").read_text())["modules"]) for platform in ("android", "ios", "vehicle"))
        self.assertEqual(report["summary"]["artifacts"], expected)
        self.assertEqual(report["summary"]["reviewed"], len(self.rules["rules"]))
        self.assertGreater(report["summary"]["needsReview"], 0)
        for entry in report["artifacts"]:
            for candidate in entry["candidates"]:
                self.assertEqual(candidate["status"], "needs_review")
