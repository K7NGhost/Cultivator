import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import os
from unittest import mock

spec = importlib.util.spec_from_file_location("vector_index", Path(__file__).resolve().parents[2] / "src-tauri/src/scripting/worker/vector_index.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class FakeEncoder:
    calls = 0
    def chunks(self, text):
        text.encode("utf-8")
        yield 0, len(text.split()), text
    def encode(self, texts):
        self.calls += len(texts)
        return [[1.0, 0.0] + [0.0] * 1022 for _ in texts]


class Vectors(unittest.TestCase):
    def test_utf8_boundary_preserves_unicode_and_escapes_only_surrogates(self):
        original = "Zoë 😀 \udc9d \ud800"
        escaped = module.utf8_text(original)
        self.assertEqual(escaped, "Zoë 😀 " + r"\udc9d \ud800")
        self.assertEqual(escaped.encode("utf-8").decode("utf-8"), escaped)

    def test_readable_artifacts_and_exact_identifiers(self):
        text = module.readable('Bluetooth\n{"device_address":"AA:BB:CC","id":12345678901234567890}', "1")
        self.assertIn("device address: AA:BB:CC", text)
        self.assertIn("12345678901234567890", text)
        self.assertEqual(module.readable('{"literal":true}', "2"), '{"literal":true}')

    @unittest.skipUnless(importlib.util.find_spec("lancedb"), "requires LanceDB integration dependencies")
    def test_completed_snapshot_skips_reconciliation_but_interrupted_pass_does_not(self):
        with tempfile.TemporaryDirectory() as root:
            encoder = FakeEncoder()
            index = module.VectorIndex(root, encoder)
            docs = [dict(group="1:row", chunk=0, stage="1", source="/messages.db", body="Original")]
            index.begin("snapshot-a")
            index.add(docs)
            index.finish()
            version = index.table.version
            self.assertTrue(index.begin("snapshot-a")["unchanged"])
            self.assertEqual(index.table.version, version)
            index.ledger.close()
            index = module.VectorIndex(root, encoder)
            self.assertTrue(index.begin("snapshot-a")["unchanged"])
            index.begin("snapshot-b")
            index.add(docs)
            index.ledger.close()
            index = module.VectorIndex(root, encoder)
            try:
                self.assertNotIn("unchanged", index.begin("snapshot-b"))
                self.assertEqual(index.add(docs)["updated"], 0)
                index.finish()
                self.assertTrue(index.begin("snapshot-b")["unchanged"])
                self.assertEqual(encoder.calls, 1)
            finally:
                index.ledger.close()

    @unittest.skipUnless(importlib.util.find_spec("lancedb"), "requires LanceDB integration dependencies")
    def test_fragment_maintenance_and_unchanged_batches_avoid_table_scans(self):
        with tempfile.TemporaryDirectory() as root:
            messages = []
            index = module.VectorIndex(root, FakeEncoder(), messages.append)
            try:
                index.begin()
                docs = [dict(group=f"2:file-{i}", chunk=0, stage="2", source="/notes.txt", body="Phone paired")
                        for i in range(128)]
                for doc in docs:
                    index.add([doc])
                self.assertIn("Compacting semantic index fragments", messages)
                self.assertLess(index.table.stats()["fragment_stats"]["num_fragments"], 128)
                self.assertEqual(index.table.count_rows(), len(docs))
                with mock.patch.object(index.table, "count_rows", side_effect=AssertionError("Unchanged batches must not scan Lance")):
                    self.assertEqual(index.add(docs)["updated"], 0)
            finally:
                index.ledger.close()

    @unittest.skipUnless(importlib.util.find_spec("lancedb"), "requires LanceDB integration dependencies")
    def test_surrogates_in_text_paths_and_artifact_json_resume_without_data_loss(self):
        with tempfile.TemporaryDirectory() as root:
            encoder = FakeEncoder()
            index = module.VectorIndex(root, encoder)
            malformed = "x" * 1070 + "\udc9d"
            artifact = json.dumps({"message": "Evidence \udc9d", "sender": "Zoë 😀"})
            docs = [dict(group="2:file", chunk=0, stage="2", source="/file\udc9d.txt", body=malformed),
                    dict(group="1:artifact", chunk=0, stage="1", source="/messages.db", body=artifact)]
            try:
                index.begin()
                self.assertEqual(index.add(docs)["updated"], 2)
                index.finish()
                stored = {row["group"]: row for row in index.table.to_arrow().to_pylist()}
                self.assertEqual(stored["2:file"]["body"], "x" * 1070 + r"\udc9d")
                self.assertEqual(stored["2:file"]["source"], r"/file\udc9d.txt")
                self.assertEqual(stored["1:artifact"]["body"], artifact)
                self.assertIn(r"message: Evidence \udc9d", stored["1:artifact"]["text"])
                self.assertIn("sender: Zoë 😀", stored["1:artifact"]["text"])
            finally:
                index.ledger.close()
            del index
            index = module.VectorIndex(root, encoder)
            try:
                index.begin()
                self.assertEqual(index.add(docs)["updated"], 0)
                index.finish()
                self.assertEqual(encoder.calls, 2)
                self.assertEqual(index.search("evidence", None, 0, 10)["total"], 2)
            finally:
                index.ledger.close()

    @unittest.skipUnless(importlib.util.find_spec("lancedb"), "requires LanceDB integration dependencies")
    def test_lancedb_persistence_reuse_replacement_filtering_and_deletion(self):
        with tempfile.TemporaryDirectory() as root:
            encoder = FakeEncoder()
            index = module.VectorIndex(root, encoder)
            docs = [dict(group="1:row'1", chunk=0, stage="1", source="/messages.db", body='Messages\n{"sender":"Alice"}'),
                    dict(group="2:file", chunk=0, stage="2", source="/notes.txt", body="Phone paired with vehicle")]
            index.begin()
            self.assertEqual(index.add(docs)["updated"], 2)
            index.finish()
            self.assertEqual(index.table.count_rows(), 2)
            self.assertEqual(encoder.calls, 2)
            index.begin()
            self.assertEqual(index.add(docs)["updated"], 0)
            index.finish()
            self.assertEqual(encoder.calls, 2)
            result = index.search("phones connected to vehicle", 1, 0, 50)
            self.assertEqual(result["total"], 1)
            self.assertEqual(result["hits"][0]["source"], "/messages.db")
            self.assertIn("tokenStart", result["hits"][0])
            # An interrupted pass must not remove records it hasn't visited.
            index.begin()
            index.add(docs[:1])
            self.assertEqual(index.table.count_rows(), 2)
            index.ledger.close()
            del index

            index = module.VectorIndex(root, encoder)
            # A cancelled query can restart the shared Python worker while a
            # native background reconciliation is still active.
            self.assertIsNotNone(index.generation)
            index.add(docs[1:])
            index.finish()
            self.assertEqual(index.table.count_rows(), 2)
            index.begin()
            docs[0]["body"] = "Updated artifact"
            self.assertEqual(index.add(docs[:1])["updated"], 1)
            index.finish()
            self.assertEqual(index.table.count_rows(), 1)
            self.assertEqual(index.table.to_arrow().to_pylist()[0]["body"], "Updated artifact")
            index.ledger.close()
            del index

    @unittest.skipUnless(os.environ.get("CULTIVATOR_BGE_TOKENIZER"), "requires downloaded BGE tokenizer fixture")
    def test_model_tokenizer_never_truncates_long_passages(self):
        from tokenizers import Tokenizer
        tokenizer = Tokenizer.from_file(os.environ["CULTIVATOR_BGE_TOKENIZER"])
        tokenizer.no_truncation()
        class Adapter:
            def __call__(self, text, **kwargs):
                return {"offset_mapping": tokenizer.encode(text, add_special_tokens=False).offsets}
            def encode(self, text, add_special_tokens=True):
                return tokenizer.encode(text, add_special_tokens=add_special_tokens).ids
        encoder = module.Encoder.__new__(module.Encoder)
        encoder.tokenizer = Adapter()
        text = "Bluetooth vehicle connection AA:BB:CC identifiers and messages. " * 1000
        chunks = list(encoder.chunks(text))
        self.assertGreater(len(chunks), 1)
        self.assertEqual(chunks[0][0], 0)
        self.assertEqual(chunks[-1][1], len(tokenizer.encode(text, add_special_tokens=False).ids))
        for previous, current in zip(chunks, chunks[1:]):
            self.assertLess(current[0], previous[1])
        for _, _, content in chunks:
            self.assertLessEqual(len(tokenizer.encode(content).ids), 512)



if __name__ == "__main__":
    unittest.main()
