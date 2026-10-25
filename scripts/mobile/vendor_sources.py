"""Vendor headless LEAPP runtimes and record source/coverage hashes.

Usage: python scripts/mobile/vendor_sources.py --ios PATH --android PATH --vehicle PATH
No source checkout is modified. Development caches and test evidence are excluded.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from inventory_sources import inspect

DESTINATION = Path(__file__).resolve().parents[2] / "src-tauri/resources/mobile"

# Cultivator consumes LAVA directly and disables upstream HTML generation.
# Keep Python compatibility/report modules: artifact scripts still import them.
REPORT_ASSET_DIRECTORIES = ("assets", "scripts/_elements")


def include_runtime_file(relative):
    relative = Path(relative)
    if "__pycache__" in relative.parts or relative.suffix in (".pyc", ".pyo"):
        return False
    name = relative.as_posix()
    return not any(name == directory or name.startswith(directory + "/")
                   for directory in REPORT_ASSET_DIRECTORIES)


def vendor(source, platform):
    source = Path(source).resolve()
    destination = DESTINATION / platform
    if (destination / "catalog.json").exists():
        raise SystemExit(f"Destination already exists; review before replacing: {destination}")
    inventory = inspect(source)
    files = []
    for directory in ("scripts", "leapp_functions", "assets", "bin", "whl_files"):
        for path in sorted((source / directory).rglob("*")):
            if path.is_file() and include_runtime_file(path.relative_to(source)):
                files.append(path)
    project = {"ios": "iLEAPP", "android": "ALEAPP", "vehicle": "VLEAPP"}[platform]
    entry = {"ios": "ileapp.py", "android": "aleapp.py", "vehicle": "vleapp.py"}[platform]
    for name in (entry, "requirements.txt", "LICENSE", "ATTRIBUTIONS.md"):
        path = source / name
        if path.is_file():
            files.append(path)
    if platform == "ios":
        files.append(source / "admin/scripts/fetch_unifiedlog_iterator.py")
    digests = {}
    for path in files:
        relative = path.relative_to(source)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
        digests[relative.as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    git = ["git", "-c", f"safe.directory={source.as_posix()}"]
    revision = subprocess.check_output([*git, "rev-parse", "HEAD"], cwd=source, text=True).strip()
    dirty = subprocess.check_output([*git, "status", "--porcelain"], cwd=source, text=True)
    metadata = {
        "platform": platform,
        "project": project,
        "repository": "https://github.com/abrignoni/" + project,
        "revision": revision,
        "sourceHadUncommittedChanges": bool(dirty.strip()),
        "packaging": {"profile": "cultivator-headless", "excludedDirectories": list(REPORT_ASSET_DIRECTORIES)},
        "counts": inventory["counts"],
        "modules": inventory["modules"],
        "files": dict(sorted(digests.items())),
    }
    text = json.dumps(metadata, indent=2, ensure_ascii=False) + "\n"
    (destination / "catalog.json").write_text(text, encoding="utf-8")
    print(f"{platform}: {len(files)} runtime files; {inventory['counts']}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ios")
    parser.add_argument("--android")
    parser.add_argument("--vehicle")
    args = parser.parse_args()
    if not any((args.ios, args.android, args.vehicle)):
        parser.error("Specify at least one source checkout")
    for platform in ("ios", "android", "vehicle"):
        source = getattr(args, platform)
        if source:
            vendor(source, platform)
