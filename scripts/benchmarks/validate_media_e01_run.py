"""Independently check originals, copies, extracted images and cache reuse after timing."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path


def load(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve(strict=True)
    manifest = load(directory / "corpus-manifest.json")
    provenance = load(directory / "image-provenance.json")
    imported = load(directory / "case-e01" / "benchmark-results.json")
    first = load(directory / "full-run-1-working-files.json")
    second = load(directory / "full-run-2-working-files.json")
    before = {f["name"]: (f["bytes"], f["mtimeTicks"]) for f in first["files"]}
    after = {f["name"]: (f["bytes"], f["mtimeTicks"]) for f in second["files"]}
    if before != after:
        raise ValueError("Working copies were changed or rewritten during the second run")
    roots = [Path(provenance["source"]), Path(provenance["copy"]), Path(imported["workingRoots"][0])]

    def verify(record):
        relative = Path(record["path"])
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("Unsafe manifest path")
        for root in roots:
            if sha256(root / relative) != record["sha256"]:
                raise ValueError(f"File differs from the original manifest: {root / relative}")

    with ThreadPoolExecutor(max_workers=8) as workers:
        list(workers.map(verify, manifest))
    for segment in provenance["segments"]:
        if sha256(Path(segment["path"])) != segment["sha256"]:
            raise ValueError("E01 changed during benchmarking")
    result = {"files":len(manifest), "bytes":sum(f["bytes"] for f in manifest),
              "originalsSha256Verified":True, "copiedCorpusSha256Verified":True,
              "extractedWorkingFilesSha256Verified":True, "e01Unchanged":True,
              "workingCopiesReusedWithoutRewriting":True,
              "firstRunWorkingFilesBefore":first["workingFilesBefore"],
              "secondRunWorkingFilesBefore":second["workingFilesBefore"],
              "workingFilesAfter":second["workingFilesAfter"], "checkedRoots":[str(p) for p in roots]}
    (directory / "post-benchmark-validation.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
