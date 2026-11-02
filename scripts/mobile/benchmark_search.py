"""Compare full-scan and indexed glob matching on a real extraction index."""
import argparse
import fnmatch
import importlib.util
import json
import os
from pathlib import Path
import re
import time

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--platform", default="ios", choices=("ios", "android", "vehicle"))
    parser.add_argument("--samples", type=int, default=40)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("mobile_worker", ROOT / "src-tauri/src/scripting/worker/mobile_artifacts.py")
    worker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(worker)
    with args.index.open(encoding="utf-8") as stream:
        entries = [{"path": item["path"], "kind": item["kind"]}
                   for item in map(json.loads, stream)]
    catalog = json.loads((ROOT / "src-tauri/resources/mobile" / args.platform / "catalog.json").read_text())
    patterns = list(dict.fromkeys(pattern for module in catalog["modules"] for artifact in module["artifacts"]
                                 for pattern in (artifact.get("paths") or []) if isinstance(pattern, str)))
    count = min(max(args.samples, 1), len(patterns))
    patterns = [patterns[i * len(patterns) // count] for i in range(count)]
    started = time.perf_counter()
    index = worker.EvidencePathIndex(entries)
    build_seconds = time.perf_counter() - started
    legacy_seconds = indexed_seconds = 0
    matches = 0
    for pattern in patterns:
        started = time.perf_counter()
        matcher = re.compile(fnmatch.translate(os.path.normcase(pattern))).match
        expected = [item for item in entries if matcher(os.path.normcase("root/") + os.path.normcase(item["path"]))]
        legacy_seconds += time.perf_counter() - started
        started = time.perf_counter()
        actual = index.match(pattern)
        indexed_seconds += time.perf_counter() - started
        if actual != expected:
            raise AssertionError("Indexed search changed matching or ordering")
        matches += len(actual)
    result = {"entries": len(entries), "sampledPatterns": count, "matches": matches,
              "identicalResults": True, "indexBuildSeconds": build_seconds,
              "legacyMatchingSeconds": legacy_seconds, "indexedMatchingSeconds": indexed_seconds,
              "matchingSpeedup": legacy_seconds / max(indexed_seconds, 1e-9), "stats": index.stats}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()
