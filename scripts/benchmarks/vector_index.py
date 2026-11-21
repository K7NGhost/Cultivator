"""Benchmark the downloaded BGE model on existing case vectors without editing the case.

Run with Cultivator's BGE Python environment. Writes a summary and temporary
benchmark indexes beside --output; evidence text is never printed or uploaded.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--limit", type=int, default=1024)
    args = parser.parse_args()
    if not 1 <= args.limit <= 4096:
        parser.error("--limit must be 1–4096")
    root = Path(__file__).resolve().parents[2]
    spec = importlib.util.spec_from_file_location("vector_worker", root / "src-tauri/src/scripting/worker/vector_index.py")
    worker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(worker)
    import lancedb
    table = lancedb.connect(str(args.case.with_name(args.case.name + ".tantivy-v1") / "vectors-bge-v1/lance")).open_table("bge_large_v1")
    sample = table.search().select(["parent", "group", "stage", "source", "body"]).limit(args.limit).to_list()
    docs = {}
    for row in sample:
        docs[row["parent"]] = dict(group=row["group"], chunk=int(row["parent"].rsplit(":", 1)[1]),
                                   stage=row["stage"], source=row["source"], body=row["body"])
    docs = list(docs.values())
    if not docs:
        raise RuntimeError("The case has no indexed passages to benchmark")
    encoder = worker.Encoder(str(args.model))
    if encoder.device != "cuda" or next(encoder.model.parameters()).device.type != "cuda":
        raise RuntimeError("This benchmark requires GPU inference; CUDA is unavailable")
    encoder.encode(["GPU benchmark warmup"] * 8)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    results = []
    for name, documents_per_write, gpu_batch in (("previous_batch_sizes", 16, 8), ("optimized_batch_sizes", 128, 32)):
        encoder.batch_size = gpu_batch
        with tempfile.TemporaryDirectory(prefix="vector-benchmark-", dir=args.output.parent) as directory:
            index = worker.VectorIndex(directory, encoder)
            try:
                index.begin("case-sample")
                started = time.perf_counter()
                for offset in range(0, len(docs), documents_per_write):
                    index.add(docs[offset:offset + documents_per_write])
                index.finish()
                seconds = time.perf_counter() - started
                results.append(dict(configuration=name, records=len(docs), passages=index.total,
                                    seconds=round(seconds, 3), records_per_second=round(len(docs) / seconds, 1)))
                print(json.dumps(results[-1]), flush=True)
            finally:
                index.ledger.close()
                del index
    report = dict(gpu=encoder.device_name, torch=encoder.torch.__version__, cuda=encoder.torch.version.cuda,
                  stages=sorted({doc["stage"] for doc in docs}), results=results)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
