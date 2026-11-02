"""Run the production JSON-lines worker against a local, already extracted tree.

Writes reports only to --output; source files are copied by LEAPP into that output.
Artifacts stream to disk so large extractions do not accumulate in runner memory.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
WORKER = ROOT / "src-tauri/src/scripting/worker"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--python", type=Path, required=True)
    parser.add_argument("--index", type=Path, help="Reuse an index from this extraction's previous run")
    parser.add_argument("--platform", choices=("ios", "android", "vehicle"), default="ios")
    args = parser.parse_args()
    source, output = args.root.resolve(strict=True), args.output.resolve()
    if not source.is_dir() or output == source or source in output.parents:
        parser.error("Output must be outside the extracted evidence tree")
    if output.exists() and any(output.iterdir()):
        parser.error("Use a fresh output directory to keep previous run reports intact")
    output.mkdir(parents=True, exist_ok=True)
    index_path = args.index.resolve(strict=True) if args.index else output / "index.ndjson"
    indexed = 0
    started = time.perf_counter()
    # Do not follow links while enumerating evidence.
    if args.index:
        with index_path.open(encoding="utf-8") as index:
            indexed = sum(1 for line in index if line.strip())
    else:
        indexed = write_index(source, index_path)
    print(json.dumps({"indexed": indexed, "seconds": time.perf_counter() - started}), flush=True)
    run_worker(args, source, output, index_path, indexed, started)


def write_index(source, index_path):
    indexed = 0
    with index_path.open("w", encoding="utf-8") as index:
        pending = [source]
        while pending:
            with os.scandir(pending.pop()) as entries:
                for entry in entries:
                    if entry.is_symlink():
                        continue
                    directory = entry.is_dir(follow_symlinks=False)
                    status = entry.stat(follow_symlinks=False)
                    relative = Path(entry.path).relative_to(source).as_posix()
                    record = {"path": entry.path, "relative": relative,
                              "kind": "directory" if directory else "file",
                              "provenance": {"source": str(source), "evidencePath": "/" + relative,
                                             "metadata": {"size": status.st_size,
                                                          "times": {"modified": {"seconds": int(status.st_mtime),
                                                                                 "nanos": 0, "basis": "utc"}}}}}
                    index.write(json.dumps(record) + "\n")
                    indexed += 1
                    if directory:
                        pending.append(Path(entry.path))
    return indexed


def run_worker(args, source, output, index_path, indexed, started):
    request = {"roots": [{"path": str(source), "indexPath": str(index_path)}],
               "outputRoot": str(output / "parser-output"), "options": {"timezone": "UTC"},
               "normalization": {"modulePath": str(WORKER / "leapp_normalization.py"),
                                 "rulesPath": str(WORKER / "leapp_normalization_rules.json"),
                                 "schemasPath": str(WORKER / "artifact_model_schemas.json")}}
    with (output / "stderr.log").open("w", encoding="utf-8") as errors, \
            (output / "events.ndjson").open("w", encoding="utf-8") as events:
        process = subprocess.Popen([str(args.python.resolve()), "-I", "-B", str(WORKER / "mobile_artifacts.py"),
                                    str(ROOT / "src-tauri/resources/mobile" / args.platform)],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors,
                                   text=True, encoding="utf-8")
        try:
            process.stdin.write(json.dumps(request) + "\n")
            process.stdin.flush()
            last_update = 0
            materialized = 0
            result = None
            for line in process.stdout:
                events.write(line)
                event = json.loads(line)
                kind = event["type"]
                if kind == "materialize":
                    # Only acknowledge readable files inside the selected source.
                    valid = all(source in Path(path).resolve().parents and Path(path).is_file()
                                for path in event["paths"])
                    process.stdin.write(json.dumps({"ok": valid, "error": "Source file unavailable"}) + "\n")
                    process.stdin.flush()
                    materialized += len(event["paths"])
                elif kind == "done":
                    result = event
                elif kind == "error":
                    print(json.dumps(event), flush=True)
                now = time.perf_counter()
                if kind == "progress" and now - last_update >= 15:
                    print(json.dumps({**event, "elapsedSeconds": round(now - started, 2)}), flush=True)
                    last_update = now
            status = process.wait()
            summary = {"exitCode": status, "elapsedSeconds": round(time.perf_counter() - started, 3),
                       "indexed": indexed, "materializedRequests": materialized, "result": result}
            (output / "run-summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
            print(json.dumps(summary), flush=True)
            if status or result is None:
                raise SystemExit(status or 1)
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()
            process.stdin.close()
            process.stdout.close()


if __name__ == "__main__":
    main()
