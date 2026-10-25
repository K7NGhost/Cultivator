"""Verify optimized E01 working copies and persisted application plugin results."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import sqlite3


def load(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    args = parser.parse_args()
    base, corpus = args.directory.resolve(), args.corpus.resolve()
    expected = load(corpus / "corpus-manifest.json")
    provenance = load(corpus / "image-provenance.json")
    image_result = load(base / "case-image/benchmark-results.json")
    roots = [Path(provenance["source"]), Path(provenance["copy"]), Path(image_result["workingRoots"][0])]
    imports = [load(path) for path in sorted(base.glob("ui-import*.json"))]
    for imported in imports:
        roots.extend(Path(p) for p in imported["imported"]["paths"])

    def verify(record):
        relative = Path(record["path"])
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("Unsafe corpus manifest path")
        for root in roots:
            path = root / relative
            if path.stat().st_size != record["bytes"] or digest(path) != record["sha256"]:
                raise ValueError(f"Image differs from manifest: {path}")

    with ThreadPoolExecutor(max_workers=8) as pool:
        list(pool.map(verify, expected))
    original_hash = provenance["segments"][0]["sha256"]
    source_paths = {Path(provenance["segments"][0]["path"]), Path(image_result["input"])}
    if any(digest(path) != original_hash for path in source_paths):
        raise ValueError("E01 changed")
    expected_by_name = {record["path"]:record["sha256"] for record in expected}
    database = base / "ui-case/case.sqlite"
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as db:
        db.row_factory = sqlite3.Row
        rows = db.execute("SELECT datasource_id, name, payload FROM media_gallery_items WHERE media_type='image'").fetchall()
        sources = {}
        for row in rows:
            item = json.loads(row["payload"])
            actual = next(field["value"] for field in item["metadata"] if field["label"] == "SHA-256")
            assert actual == expected_by_name[row["name"]], row["name"]
            assert item["faceCount"] is not None and item["nsfwScore"] is not None
            source = sources.setdefault(row["datasource_id"], {"photos":0,"facesAnalyzed":0,"nsfwAnalyzed":0,"clipLabeled":0})
            source["photos"] += 1
            source["facesAnalyzed"] += 1
            source["nsfwAnalyzed"] += 1
            source["clipLabeled"] += bool(item["clipClassifications"])
        for source in sources.values():
            assert source["photos"] == len(expected) and source["clipLabeled"] == 912
        jobs = [dict(row) for row in db.execute("SELECT * FROM plugin_jobs ORDER BY started_at")]
        for job in jobs:
            assert job["status"] == "complete", job
            job["elapsedMs"] = int(job["finished_at"]) - int(job["execution_started_at"] or job["started_at"])
        logs = [dict(row) for row in db.execute("SELECT job_id,level,message,created_at FROM plugin_logs ORDER BY created_at")]
        requests = [dict(row) for row in db.execute("SELECT * FROM plugin_runs")]
    result = {"filesPerSource":len(expected), "bytesPerSource":sum(f["bytes"] for f in expected),
              "sha256VerifiedRoots":[str(root) for root in roots], "unchangedE01s":[str(p) for p in source_paths],
              "persistedSources":sources,"jobs":jobs,"logs":logs,"requests":requests}
    (base / "post-validation.json").write_text(json.dumps(result,indent=2)+"\n",encoding="utf-8")
    print(json.dumps({"filesPerSource":len(expected),"verifiedRoots":len(roots),"sources":sources,
                      "jobs":[{"id":job["id"],"ms":job["elapsedMs"]} for job in jobs]},indent=2))


if __name__ == "__main__":
    main()
