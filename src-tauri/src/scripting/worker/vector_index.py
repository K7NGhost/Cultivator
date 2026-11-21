"""Local, restart-safe BGE/LanceDB retrieval. JSON-lines IPC, no model downloads."""
import hashlib
import json
import os
import re
import sqlite3
import sys
import uuid
from pathlib import Path

MODEL = "BAAI/bge-large-en-v1.5@d4aa6901d3a41ba39fb536a557fa166f842b0e09"
INSTRUCTION = "Represent this sentence for searching relevant passages: "


def utf8_text(text):
    """Keep valid Unicode and visibly escape lone surrogates at UTF-8 boundaries."""
    return text.encode("utf-8", "backslashreplace").decode("utf-8")


def readable(body, stage):
    if str(stage) == "2":
        return body
    prefix, separator, rest = body.partition("\n")
    try:
        value = json.loads(body)
        prefix = ""
    except (ValueError, RecursionError):
        try:
            value = json.loads(rest) if separator else None
        except (ValueError, RecursionError):
            value = None
    if value is None:
        return body  # Partial JSON remains evidence, never invented fields.
    lines = [prefix] if prefix else []

    def walk(item, path):
        if isinstance(item, dict):
            for key, child in item.items():
                name = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", key).replace("_", " ")
                walk(child, path + [name])
        elif isinstance(item, list):
            for index, child in enumerate(item):
                walk(child, path + [str(index + 1)])
        else:
            lines.append(" / ".join(path) + ": " + str(item))
    walk(value, [])
    return "\n".join(lines)


class Encoder:
    def __init__(self, directory, progress=None):
        import torch
        from transformers import AutoModel, AutoTokenizer
        self.torch = torch
        torch.set_num_threads(4)
        self.device = "cuda" if torch.cuda.is_available() else "cpu"
        if os.environ.get("CULTIVATOR_REQUIRE_CUDA") == "1" and self.device != "cuda":
            raise RuntimeError("NVIDIA GPU detected, but BGE cannot initialize CUDA. Repair the CUDA PyTorch runtime before indexing.")
        self.device_name = torch.cuda.get_device_name(0) if self.device == "cuda" else "CPU"
        self.batch_size = 32 if self.device == "cuda" else 8
        self.progress = progress
        self.tokenizer = AutoTokenizer.from_pretrained(directory, local_files_only=True, trust_remote_code=False)
        self.model = AutoModel.from_pretrained(directory, local_files_only=True, trust_remote_code=False, use_safetensors=True).to(self.device).eval()
        if self.device == "cuda":
            self.model.half()

    def chunks(self, text):
        encoded = self.tokenizer(text, add_special_tokens=False, return_offsets_mapping=True, truncation=False)
        offsets = encoded["offset_mapping"]
        start = 0
        while start < len(offsets):
            end = min(start + 480, len(offsets))
            # A slice can start inside a WordPiece word; re-tokenization can
            # change its length. Check the actual model input, including CLS/SEP.
            while end > start and len(self.tokenizer.encode(text[offsets[start][0]:offsets[end - 1][1]], add_special_tokens=True)) > 512:
                end -= 1
            if end == start:
                raise ValueError("Cannot split document within model token limit")
            yield start, end, text[offsets[start][0]:offsets[end - 1][1]]
            if end == len(offsets):
                break
            start = max(start + 1, end - 48)

    def encode(self, texts):
        texts = [utf8_text(text) for text in texts]
        vectors = []
        batch = self.batch_size
        index = 0
        while index < len(texts):
            part = texts[index:index + batch]
            tokens = self.tokenizer(part, padding=True, truncation=False, return_tensors="pt")
            if tokens["input_ids"].shape[1] > 512:
                raise ValueError("Embedding input exceeds 512 tokens; no silent truncation is allowed")
            try:
                with self.torch.inference_mode():
                    state = self.model(**{k: v.to(self.device) for k, v in tokens.items()}).last_hidden_state[:, 0]
                    output = self.torch.nn.functional.normalize(state.float(), p=2, dim=1).cpu().numpy()
                if output.shape[1] != 1024:
                    raise ValueError("Wrong embedding dimensions")
                vectors.extend(output.tolist())
                index += len(part)
                if self.progress:
                    self.progress(f"Embedding on {self.device_name}: {index}/{len(texts)} passages")
            except self.torch.cuda.OutOfMemoryError:
                if batch == 1:
                    raise
                batch = max(1, batch // 2)
                self.batch_size = batch
                self.torch.cuda.empty_cache()
        return vectors


def quoted(value):
    return "'" + value.replace("'", "''") + "'"


class VectorIndex:
    def __init__(self, root, encoder, progress=None):
        import lancedb
        import pyarrow as pa
        root = Path(root)
        root.mkdir(parents=True, exist_ok=True)
        self.encoder = encoder
        self.progress = progress or (lambda message: None)
        self.db = lancedb.connect(str(root / "lance"))
        self.table = self.db.create_table("bge_large_v1", schema=pa.schema([
            ("id", pa.string()), ("parent", pa.string()), ("group", pa.string()),
            ("stage", pa.string()), ("source", pa.string()), ("body", pa.string()),
            ("text", pa.string()), ("chunk", pa.int64()), ("token_start", pa.int64()),
            ("token_end", pa.int64()), ("model", pa.string()), ("vector", pa.list_(pa.float32(), 1024)),
        ]), exist_ok=True)
        self.ledger = sqlite3.connect(root / "checkpoints.sqlite")
        self.ledger.execute("PRAGMA journal_mode=WAL")
        self.ledger.execute("CREATE TABLE IF NOT EXISTS parents(id TEXT PRIMARY KEY,hash TEXT,generation TEXT)")
        self.ledger.execute("CREATE TABLE IF NOT EXISTS run_state(key TEXT PRIMARY KEY,value TEXT)")
        # Missing Lance data cannot reuse fingerprints from a previous table.
        self.total = self.table.count_rows()
        if self.total == 0:
            self.ledger.execute("DELETE FROM parents")
            self.ledger.execute("DELETE FROM run_state WHERE key='completed_snapshot'")
            self.ledger.commit()
        saved = self.ledger.execute("SELECT value FROM run_state WHERE key='generation'").fetchone()
        self.generation = saved[0] if saved else None
        saved = self.ledger.execute("SELECT value FROM run_state WHERE key='snapshot'").fetchone()
        self.snapshot = saved[0] if saved else None
        self.changed = 0
        self.writes = 0

    def compact(self):
        # Small appends otherwise produce thousands of fragments, making each
        # delete and search scan increasingly expensive during the first pass.
        if self.table.stats()["fragment_stats"]["num_fragments"] >= 128:
            self.progress("Compacting semantic index fragments")
            self.table.optimize(compaction_options={"target_rows_per_fragment": 65536})
        self.writes = 0

    def begin(self, snapshot=None):
        completed = self.ledger.execute("SELECT value FROM run_state WHERE key='completed_snapshot'").fetchone()
        if snapshot is not None and not self.generation and completed and completed[0] == snapshot:
            return {"unchanged": True, "total": self.total}
        self.compact()
        self.generation = uuid.uuid4().hex
        self.snapshot = snapshot
        self.changed = 0
        self.ledger.execute("INSERT INTO run_state VALUES ('generation',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (self.generation,))
        self.ledger.execute("DELETE FROM run_state WHERE key='snapshot'")
        if snapshot is not None:
            self.ledger.execute("INSERT INTO run_state VALUES ('snapshot',?)", (snapshot,))
        self.ledger.commit()
        return {}

    def add(self, documents):
        if not self.generation:
            raise ValueError("No active reconciliation")
        changed = []
        pending = []
        rows = []
        self.progress("Checking semantic fingerprints and preparing passages")
        for doc in documents:
            group = utf8_text(doc["group"])
            parent = group + ":" + str(doc["chunk"])
            # Hash the original code points, including malformed parser text.
            # surrogatepass is only for hashing, never Arrow or model input.
            digest = hashlib.sha256((MODEL + "representation-v1" + doc["source"] + doc["body"]).encode("utf-8", "surrogatepass")).hexdigest()
            saved = self.ledger.execute("SELECT hash FROM parents WHERE id=?", (parent,)).fetchone()
            if not saved or saved[0] != digest:
                # JSON decoding can introduce lone surrogates even when the
                # indexed JSON itself is valid UTF-8. Escape them for the model;
                # preserve the indexed body and ordinary Unicode verbatim.
                text = utf8_text(readable(doc["body"], doc["stage"]))
                chunks = list(self.encoder.chunks(text))
                source, body = utf8_text(doc["source"]), utf8_text(doc["body"])
                rows.extend(dict(id=parent + ":" + str(i), parent=parent, group=group,
                             stage=doc["stage"], source=source, body=body, text=chunk[2],
                             chunk=i, token_start=chunk[0], token_end=chunk[1], model=MODEL)
                        for i, chunk in enumerate(chunks))
                changed.append(parent)
            pending.append((parent, digest, self.generation))
        # Lance commits before SQLite checkpoints. Replays replace whole parent groups.
        if changed:
            vectors = self.encoder.encode([row["text"] for row in rows]) if rows else []
            for row, vector in zip(rows, vectors):
                row["vector"] = vector
            self.progress(f"Writing {len(rows)} semantic passages")
            self.table.delete("parent IN (" + ",".join(quoted(parent) for parent in changed) + ")")
            if rows:
                self.table.add(rows)
        self.ledger.executemany("INSERT INTO parents VALUES (?,?,?) ON CONFLICT(id) DO UPDATE SET hash=excluded.hash,generation=excluded.generation", pending)
        self.ledger.commit()
        self.changed += len(changed)
        if changed:
            self.total = self.table.count_rows()
            self.writes += 1
            if self.writes >= 128:
                self.compact()
        return {"updated": len(changed), "total": self.total}

    def finish(self):
        if not self.generation:
            raise ValueError("No active reconciliation")
        while True:
            stale = self.ledger.execute("SELECT id FROM parents WHERE generation<>? LIMIT 1024", (self.generation,)).fetchall()
            if not stale:
                break
            self.progress("Removing stale semantic passages")
            self.table.delete("parent IN (" + ",".join(quoted(row[0]) for row in stale) + ")")
            self.ledger.executemany("DELETE FROM parents WHERE id=?", stale)
            self.ledger.commit()
        total = self.table.count_rows()
        self.compact()
        if total >= 4096 and not self.table.list_indices():
            self.progress("Building semantic search index")
            self.table.create_index(metric="cosine", num_partitions=max(1, min(256, total // 256)), num_sub_vectors=64)
        self.generation = None
        self.total = total
        self.ledger.execute("DELETE FROM run_state WHERE key IN ('generation','snapshot')")
        if self.snapshot is not None:
            self.ledger.execute("INSERT INTO run_state VALUES ('completed_snapshot',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (self.snapshot,))
        self.ledger.commit()
        return {"total": total}

    def search(self, query, stage, offset, limit):
        if not query.strip() or len(query) > 4096 or offset > 1000 or not 1 <= limit <= 100 or stage not in (None, 0, 1, 2):
            raise ValueError("Invalid semantic query or page")
        vector = self.encoder.encode([INSTRUCTION + query])[0]
        where = "stage = " + quoted(str(stage)) if stage is not None else None
        search = self.table.search(vector).metric("cosine").refine_factor(5)
        if where:
            search = search.where(where, prefilter=True)
        rows = search.limit(min(100, offset + limit)).to_list()
        hits = [dict(group=row["parent"], recordId=row["group"], source=row["source"], stage=row["stage"], body=row["body"],
                     chunk=row["chunk"], tokenStart=row["token_start"], tokenEnd=row["token_end"],
                     distance=row["_distance"]) for row in rows[offset:]]
        return {"ready": True, "total": min(100, self.table.count_rows(where)), "hits": hits}


def main():
    def progress(message):
        print(json.dumps({"progress": message}), flush=True)
    progress("Loading local BGE model")
    encoder = Encoder(sys.argv[1], progress)
    index = VectorIndex(sys.argv[2], encoder, progress)
    print(json.dumps({"ready": True, "device": encoder.device, "deviceName": encoder.device_name}), flush=True)
    for line in sys.stdin:
        try:
            request = json.loads(line)
            op = request["op"]
            if op == "begin":
                response = index.begin(request.get("snapshot"))
            elif op == "add":
                response = index.add(request["documents"])
            elif op == "finish":
                response = index.finish()
            elif op == "search":
                response = index.search(request["query"], request.get("stage"), request["offset"], request["limit"])
            else:
                raise ValueError("Unknown operation")
            print(json.dumps(response), flush=True)
        except Exception as error:
            print(json.dumps({"error": str(error)}), flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(json.dumps({"error": str(error)}), flush=True)
