"""Verified, resumable model downloads. Uses only the Python standard library."""
import hashlib
import json
import os
from pathlib import Path
import sys
import time
import urllib.request
import zipfile


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def destination(root, relative):
    path = (root / relative).resolve()
    if not path.is_relative_to(root.resolve()) or path == root.resolve():
        raise ValueError("Invalid model file path")
    return path


def verified(path, size, sha256):
    return path.is_file() and path.stat().st_size == size and digest(path) == sha256


def download_file(root, file, report):
    target = destination(root, file["path"])
    target.parent.mkdir(parents=True, exist_ok=True)
    report(0, "Verifying " + file["path"])
    size = file.get("downloadSize", file["size"])
    if verified(target, file["size"], file["sha256"]):
        report(size, "Already downloaded: " + file["path"])
        return
    partial = target.with_name(target.name + ".part")
    expected_hash = file.get("downloadSha256", file["sha256"])
    offset = partial.stat().st_size if partial.is_file() else 0
    if offset > size:
        partial.unlink()
        offset = 0
    if offset < size:
        request = urllib.request.Request(file["url"], headers={"User-Agent": "Cultivator-Model-Manager/1", **({"Range": f"bytes={offset}-"} if offset else {})})
        with urllib.request.urlopen(request, timeout=30) as response:
            if offset and (response.status != 206 or not response.headers.get("Content-Range", "").startswith(f"bytes {offset}-")):
                offset = 0
            mode = "ab" if offset else "wb"
            report(offset, "Downloading " + file["path"])
            last_report = 0
            with partial.open(mode) as stream:
                while block := response.read(256 * 1024):
                    offset += len(block)
                    if offset > size:
                        raise ValueError("Download exceeds expected model size")
                    stream.write(block)
                    now = time.monotonic()
                    if now - last_report >= 0.2:
                        report(offset, "Downloading " + file["path"])
                        last_report = now
    report(size, "Verifying " + file["path"])
    if not verified(partial, size, expected_hash):
        partial.unlink(missing_ok=True)
        raise ValueError("Model download failed SHA-256 verification: " + file["path"])
    if file.get("archiveMember"):
        # Extract only the exact known checkpoint, never arbitrary archive paths.
        extracted = target.with_name(target.name + ".extracting")
        try:
            with zipfile.ZipFile(partial) as archive:
                member = archive.getinfo(file["archiveMember"])
                if member.file_size != file["size"]:
                    raise ValueError("Unexpected checkpoint size in model archive")
                with archive.open(member) as source, extracted.open("wb") as output:
                    while block := source.read(256 * 1024):
                        output.write(block)
            if not verified(extracted, file["size"], file["sha256"]):
                raise ValueError("Extracted checkpoint failed SHA-256 verification")
            os.replace(extracted, target)
            partial.unlink()
        finally:
            extracted.unlink(missing_ok=True)
    else:
        os.replace(partial, target)


def download(root, catalog, model_id, emit):
    models = {model["id"]: model for model in catalog}
    ordered = []
    visiting = set()

    def visit(identifier):
        if identifier in visiting:
            raise ValueError("Cyclic model dependency")
        if identifier in [model["id"] for model in ordered]:
            return
        visiting.add(identifier)
        model = models[identifier]
        for dependency in model["dependencies"]:
            visit(dependency)
        visiting.remove(identifier)
        ordered.append(model)

    visit(model_id)
    total = sum(file.get("downloadSize", file["size"]) for model in ordered for file in model["files"])
    completed = 0
    for model in ordered:
        directory = destination(root, model["id"])
        directory.mkdir(parents=True, exist_ok=True)
        receipt = {"files": {}}
        for file in model["files"]:
            def report(count, message):
                emit({"downloaded": completed + count, "total": total, "message": message})
            download_file(directory, file, report)
            path = destination(directory, file["path"])
            stat = path.stat()
            receipt["files"][file["path"]] = {"sha256": file["sha256"], "size": stat.st_size, "modified_ns": stat.st_mtime_ns}
            completed += file.get("downloadSize", file["size"])
        temporary = directory / ".ready.json.part"
        temporary.write_text(json.dumps(receipt), encoding="utf-8")
        os.replace(temporary, directory / ".ready.json")
    emit({"downloaded": total, "total": total, "message": "Download verified and ready"})


if __name__ == "__main__":
    def emit(value):
        print(json.dumps(value), flush=True)
    try:
        download(Path(sys.argv[1]), json.load(sys.stdin), sys.argv[2], emit)
    except Exception as error:
        emit({"error": str(error)})
        sys.exit(1)
