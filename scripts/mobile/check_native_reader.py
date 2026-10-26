"""Download, verify and smoke-test the optional native Unified Logs reader.

Run with the iOS plugin's Python environment. Outputs stay in the supplied cache.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", required=True, type=Path)
    args = parser.parse_args()
    project = Path(__file__).resolve().parents[2]
    bundle = project / "src-tauri/resources/mobile/ios"
    sys.path.insert(0, str(bundle))
    helper = Path("admin/scripts/fetch_unifiedlog_iterator.py")
    destination = args.cache.resolve()
    (destination / helper).parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(bundle / helper, destination / helper)
    spec = importlib.util.spec_from_file_location("mobile_adapter", project / "src-tauri/src/scripting/worker/mobile_artifacts.py")
    adapter = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(adapter)
    from scripts import unifiedlogs
    # Exercise the verified provisioning path independently of tools on PATH.
    unifiedlogs.find_iterator = lambda: None
    adapter.install_unified_log_reader(destination)
    binary = Path(unifiedlogs.find_iterator())
    first = hashlib.sha256(binary.read_bytes()).hexdigest()
    assert Path(unifiedlogs.find_iterator()) == binary
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == first
    result = subprocess.run([str(binary), "--version"], capture_output=True, text=True, timeout=15)
    version = (result.stdout + result.stderr).strip()
    if "0.7.0" not in version:
        raise SystemExit(f"Unexpected native reader response: {version}")
    summary = {"binary": str(binary), "sha256": first, "version": version, "cacheReuseVerified": True}
    (destination / "smoke-test.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
