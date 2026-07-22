"""Generate native Cultivator plugins from every iLEAPP scripts/artifacts item."""

from __future__ import annotations

import argparse
import ast
import json
import os
import pprint
import re
import shutil
from pathlib import Path
from typing import Any

from inventory import build_inventory


EXPECTED_SOURCE_FILES = 276
EXPECTED_PYTHON_MODULES = 274
EXPECTED_SUPPORT_FILES = {"NotificationParams.txt", "script.txt"}
ARCHIVE_PATH_GLOB_OVERRIDES = {
    # This artifact describes the Info.plist at an iTunes backup root. Its
    # runtime glob intentionally remains broad enough to find the extracted
    # file, while selective archive extraction must not copy every app and
    # framework Info.plist from a full-filesystem image.
    "iTunesBackupInfo.py": ("root/Info.plist",),
}
SUPPORT_FILES = (
    "builds_ids.py",
    "ccl_leveldb.py",
    "ccl_simplesnappy.py",
    "chat_rendering.py",
    "filetype.py",
    "parse3.py",
)
SUPPORT_DIRECTORIES = ("ccl", "ccl_segb", "filetypes", "ktx")
SOURCE_REPLACEMENTS = (
    ("scripts.artifact_report", "cultivator_native"),
    ("scripts.ilapfuncs", "cultivator_native"),
    ("scripts.lavafuncs", "cultivator_native"),
    ("scripts.builds_ids", "cultivator_support.builds_ids"),
    ("scripts.ccl_leveldb", "cultivator_support.ccl_leveldb"),
    ("scripts.ccl_segb", "cultivator_support.ccl_segb"),
    ("scripts.ccl", "cultivator_support.ccl"),
    ("scripts.chat_rendering", "cultivator_support.chat_rendering"),
    ("scripts.filetype", "cultivator_support.filetype"),
    ("scripts.ktx", "cultivator_support.ktx"),
    ("scripts.parse3", "cultivator_support.parse3"),
    ("from scripts import", "from cultivator_support import"),
    ("ArtifactHtmlReport", "CultivatorTableReport"),
)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="Path to the iLEAPP repository root")
    parser.add_argument(
        "output",
        nargs="?",
        type=Path,
        help=(
            "Destination for the native iLEAPP folder; defaults to Cultivator's "
            "plugins/python/iLEAPP program-data folder"
        ),
    )
    return parser.parse_args()


def default_plugin_output() -> Path:
    appdata = os.environ.get("APPDATA")
    if not appdata:
        raise RuntimeError("APPDATA is not set; provide an explicit output path")
    return Path(appdata) / "com.k7nghost.cultivator" / "plugins" / "python" / "iLEAPP"


def slugify(value: str) -> str:
    slug = re.sub(r"[^0-9A-Za-z]+", "-", value).strip("-").lower()
    return slug or "artifact"


def transform_source(source: str) -> str:
    converted = source
    for old, new in SOURCE_REPLACEMENTS:
        converted = converted.replace(old, new)
    converted = re.sub(
        r"(?m)^([ \t]*)import ccl_bplist\s*$",
        r"\1from cultivator_support.ccl import ccl_bplist",
        converted,
    )
    return converted


def bootstrap_source() -> str:
    return '''# Generated native Cultivator port; parser logic retained under the iLEAPP MIT license.
import sys as _cultivator_sys
from pathlib import Path as _CultivatorPluginPath

_CULTIVATOR_SHARED = _CultivatorPluginPath(__file__).resolve().parent.parent / "_shared"
if str(_CULTIVATOR_SHARED) not in _cultivator_sys.path:
    _cultivator_sys.path.insert(0, str(_CULTIVATOR_SHARED))

import cultivator_api
from cultivator_native import run_module as _cultivator_run_module

'''


def entrypoint_source(artifacts: list[dict[str, Any]]) -> str:
    specs = pprint.pformat(artifacts, width=100, sort_dicts=True)
    return f'''

_CULTIVATOR_ARTIFACTS = {specs}
_CULTIVATOR_HAS_RUN = False


def run(context):
    """Run every registered parser from this source module once per datasource."""
    global _CULTIVATOR_HAS_RUN
    if _CULTIVATOR_HAS_RUN:
        return None
    _CULTIVATOR_HAS_RUN = True
    _cultivator_run_module(context, globals(), _CULTIVATOR_ARTIFACTS)
    return None
'''


def toml_string(value: Any) -> str:
    return json.dumps(str(value), ensure_ascii=False)


def manifest_text(module: dict[str, Any], slug: str) -> str:
    artifacts = module["artifacts"]
    names = [artifact["name"] for artifact in artifacts]
    name = names[0] if len(names) == 1 else f"{Path(module['file']).stem} ({len(names)} artifacts)"
    descriptions = list(
        dict.fromkeys(artifact["description"].strip() for artifact in artifacts if artifact["description"].strip())
    )
    description = " ".join(descriptions) or f"Native Cultivator port of iLEAPP {Path(module['file']).stem}."
    authors = list(dict.fromkeys(artifact["author"].strip() for artifact in artifacts if artifact["author"].strip()))
    author = "; ".join(authors) or "iLEAPP contributors"
    patterns = sorted(
        dict.fromkeys(
            pattern.replace("\\", "/")
            for artifact in artifacts
            for pattern in artifact["paths"]
        )
    )
    path_glob = ",\n  ".join(toml_string(pattern) for pattern in patterns)
    archive_patterns = ARCHIVE_PATH_GLOB_OVERRIDES.get(Path(module["file"]).name, ())
    archive_path_glob = ""
    if archive_patterns:
        rendered_archive_patterns = ",\n  ".join(
            toml_string(pattern) for pattern in archive_patterns
        )
        archive_path_glob = f"archive_path_glob = [\n  {rendered_archive_patterns}\n]\n"
    return f'''id = {toml_string(f"ileapp-{slug}")}
name = {toml_string(name)}
author = {toml_string(author)}
version = "1.0.0"
description = {toml_string(description)}
type = "other"
target = "ios"
mode = "path_glob"
path_glob = [
  {path_glob}
]
{archive_path_glob}entry = "plugin.py"
function = "run"
'''


def extract_function(source_path: Path, function_name: str) -> str:
    source = source_path.read_text(encoding="utf-8-sig")
    tree = ast.parse(source, filename=str(source_path))
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == function_name:
            segment = ast.get_source_segment(source, node)
            if segment:
                return segment
    raise RuntimeError(f"Function '{function_name}' was not found in {source_path}")


def write_device_resolutions(source_scripts: Path, native_directory: Path) -> None:
    function = extract_function(source_scripts / "ilapfuncs.py", "get_resolution_for_model_id")
    content = (
        '"""Device resolution data retained from iLEAPP for photo analysis parsers."""\n\n'
        "from .io_helpers import logfunc\n\n\n"
        + function
        + "\n"
    )
    (native_directory / "device_resolutions.py").write_text(content, encoding="utf-8")


def copy_support_code(source_scripts: Path, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "__init__.py").write_text(
        '"""Parser support code bundled with the native iLEAPP Cultivator port."""\n',
        encoding="utf-8",
    )
    for filename in SUPPORT_FILES:
        shutil.copy2(source_scripts / filename, destination / filename)
    for directory_name in SUPPORT_DIRECTORIES:
        shutil.copytree(
            source_scripts / directory_name,
            destination / directory_name,
            ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
        )
    for path in destination.rglob("*.py"):
        source = path.read_text(encoding="utf-8-sig", errors="replace")
        path.write_text(source.replace("scripts.", "cultivator_support."), encoding="utf-8")


def copy_runtime_requirements(source_root: Path, shared_directory: Path) -> None:
    requirements = '''astc_decomp_faster
bencoding
biplist
blackboxprotobuf
beautifulsoup4
ijson
mmh3
mdplistlib
nska-deserialize>=1.3.1
numpy
packaging==24.1
pandas
PGPy
pillow
pillow_heif
pycryptodome
pytz
'''
    (shared_directory / "requirements.txt").write_text(requirements, encoding="utf-8")
    wheel = source_root / "whl_files" / "pyliblzfse-0.4.1-cp312-cp312-win_amd64.whl"
    wheels = shared_directory / "wheels"
    wheels.mkdir(parents=True, exist_ok=True)
    shutil.copy2(wheel, wheels / wheel.name)


def readme_text() -> str:
    return """# Native iLEAPP plugins for Cultivator

This generated set contains one native Cultivator plugin for each of the 274 Python modules in iLEAPP `scripts/artifacts`. The two text assets from that directory are carried under `_shared/assets` and copied beside the notification parser that consumes them.

The parser implementations do not import iLEAPP at runtime. Their imports and entrypoints are rewritten to use `cultivator_api` and the bundled native parsing utilities. Each emitted custom table explicitly groups identical displayed rows by all displayed fields, retaining occurrence data while preventing duplicate visible entries.

The original parser code and bundled support code remain licensed under iLEAPP's MIT license. See `LICENSE.iLEAPP` and `coverage.json`.
"""


def build_plugins(source_root: Path, output: Path) -> dict[str, Any]:
    artifacts_source = source_root / "scripts" / "artifacts"
    inventory = build_inventory(artifacts_source)
    if inventory["sourceFileCount"] != EXPECTED_SOURCE_FILES:
        raise RuntimeError(f"Expected {EXPECTED_SOURCE_FILES} source files, found {inventory['sourceFileCount']}")
    if inventory["pythonModuleCount"] != EXPECTED_PYTHON_MODULES:
        raise RuntimeError(f"Expected {EXPECTED_PYTHON_MODULES} Python modules, found {inventory['pythonModuleCount']}")
    if set(inventory["supportFiles"]) != EXPECTED_SUPPORT_FILES:
        raise RuntimeError(f"Unexpected support-file inventory: {inventory['supportFiles']}")
    if inventory["parseErrors"] or inventory["modulesWithoutMetadata"]:
        raise RuntimeError("Every source module must parse and contain artifact metadata")

    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    shared = output / "_shared"
    template_runtime = Path(__file__).resolve().parent / "templates" / "cultivator_native"
    shutil.copytree(template_runtime, shared / "cultivator_native")
    write_device_resolutions(source_root / "scripts", shared / "cultivator_native")
    copy_support_code(source_root / "scripts", shared / "cultivator_support")
    copy_runtime_requirements(source_root, shared)

    assets = shared / "assets"
    assets.mkdir(parents=True)
    for filename in sorted(EXPECTED_SUPPORT_FILES):
        shutil.copy2(artifacts_source / filename, assets / filename)

    coverage_modules = []
    slugs: set[str] = set()
    for module in inventory["modules"]:
        source_file = artifacts_source / module["file"]
        slug = slugify(source_file.stem)
        if slug in slugs:
            raise RuntimeError(f"Duplicate generated plugin slug: {slug}")
        slugs.add(slug)
        plugin_directory = output / slug
        plugin_directory.mkdir()
        converted = transform_source(source_file.read_text(encoding="utf-8-sig", errors="replace"))
        plugin_source = bootstrap_source() + converted.rstrip() + "\n" + entrypoint_source(module["artifacts"])
        (plugin_directory / "plugin.py").write_text(plugin_source, encoding="utf-8")
        (plugin_directory / "plugin.toml").write_text(manifest_text(module, slug), encoding="utf-8")
        if module["file"] == "notificationsXI.py":
            for filename in sorted(EXPECTED_SUPPORT_FILES):
                shutil.copy2(artifacts_source / filename, plugin_directory / filename)
        coverage_modules.append(
            {
                "sourceFile": module["file"],
                "pluginDirectory": slug,
                "pluginId": f"ileapp-{slug}",
                "registeredArtifacts": [artifact["id"] for artifact in module["artifacts"]],
            }
        )

    coverage = {
        "schemaVersion": 1,
        "source": "iLEAPP/scripts/artifacts",
        "sourceFileCount": inventory["sourceFileCount"],
        "pythonModuleCount": inventory["pythonModuleCount"],
        "registeredArtifactCount": inventory["registeredArtifactCount"],
        "supportFiles": [
            {"sourceFile": filename, "output": f"_shared/assets/{filename}"}
            for filename in sorted(EXPECTED_SUPPORT_FILES)
        ],
        "modules": coverage_modules,
    }
    (output / "coverage.json").write_text(
        json.dumps(coverage, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    (output / "README.md").write_text(readme_text(), encoding="utf-8")
    shutil.copy2(source_root / "LICENSE", output / "LICENSE.iLEAPP")
    return coverage


def main() -> int:
    arguments = parse_arguments()
    source = arguments.source.resolve()
    output = (arguments.output or default_plugin_output()).resolve()
    if not (source / "scripts" / "artifacts").is_dir():
        raise SystemExit(f"Not an iLEAPP source tree: {source}")
    coverage = build_plugins(source, output)
    print(
        json.dumps(
            {
                "output": str(output),
                "sourceFileCount": coverage["sourceFileCount"],
                "pythonModuleCount": coverage["pythonModuleCount"],
                "registeredArtifactCount": coverage["registeredArtifactCount"],
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
