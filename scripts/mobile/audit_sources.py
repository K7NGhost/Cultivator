"""Verify bundled sources against their original checkouts and Git exclusions."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from vendor_sources import REPORT_ASSET_DIRECTORIES, include_runtime_file
from runtime_policy import EXCLUDED_MODULES, filter_requirements

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ios", type=Path)
    parser.add_argument("--android", type=Path)
    parser.add_argument("--vehicle", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    summary, paths = {}, []
    if not any((args.ios, args.android, args.vehicle)):
        parser.error("Specify at least one source checkout")
    for platform in ("ios", "android", "vehicle"):
        source = getattr(args, platform)
        if source is None:
            continue
        bundle = ROOT / "src-tauri/resources/mobile" / platform
        catalog = json.loads((bundle / "catalog.json").read_text(encoding="utf-8"))
        for directory in REPORT_ASSET_DIRECTORIES:
            if (bundle / directory).exists():
                raise SystemExit(f"Unused report assets bundled: {bundle / directory}")
        for relative, expected in catalog["files"].items():
            if not include_runtime_file(relative, platform):
                raise SystemExit(f"Excluded resource in runtime catalog: {relative}")
            for path in (source / relative, bundle / relative):
                content = path.read_bytes()
                if path == source / relative and relative == "requirements.txt":
                    if hashlib.sha256(content).hexdigest() != catalog["sourceOverrides"][relative]:
                        raise SystemExit(f"Upstream requirements changed: {path}")
                    content = filter_requirements(content.decode("utf-8"), platform).encode("utf-8")
                elif path == source / relative and relative in catalog.get("sourceOverrides", {}):
                    # Local runtime fixes retain the original source hash as well
                    # as the patched bundle hash in files.
                    if hashlib.sha256(content).hexdigest() != catalog["sourceOverrides"][relative]:
                        raise SystemExit(f"Upstream source changed: {path}")
                    continue
                actual = hashlib.sha256(content).hexdigest()
                if actual != expected:
                    raise SystemExit(f"Source hash mismatch: {path}")
            paths.append((bundle / relative).relative_to(ROOT).as_posix())
        # Check both definition directories independently of the recorded list.
        original = {p.relative_to(source).as_posix() for folder in ("artifacts", "alternate_artifacts")
                    for p in (source / "scripts" / folder).rglob("*.py") if p.name != "__init__.py"}
        recorded = {module["path"] for module in catalog["modules"]}
        excluded = original.intersection(EXCLUDED_MODULES[platform])
        if original - excluded != recorded:
            raise SystemExit(f"{platform}: artifact source files omitted or added: {(original - excluded) ^ recorded}")
        summary[platform] = {"revision": catalog["revision"], "runtimeFiles": len(catalog["files"]),
                             "artifactDefinitions": sum(len(module["artifacts"]) for module in catalog["modules"]),
                             "sourceHashesVerified": True, "moduleFilesVerified": True}
        paths.append((bundle / "catalog.json").relative_to(ROOT).as_posix())
    result = subprocess.run(["git", "-c", f"safe.directory={ROOT.as_posix()}", "check-ignore", "--no-index", "--stdin", "-z"],
                            cwd=ROOT, input="\0".join(paths).encode("utf-8") + b"\0", capture_output=True)
    if result.returncode != 1 or result.stdout:
        raise SystemExit(f"Bundled files would be ignored by Git: {result.stdout!r} {result.stderr!r}")
    summary["gitVisibilityVerified"] = True
    text = json.dumps(summary, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text, encoding="utf-8")
    print(text)


if __name__ == "__main__":
    main()
