"""Validate generated native iLEAPP Cultivator plugin coverage and loading."""

from __future__ import annotations

import argparse
import ast
import importlib.util
import json
import re
import sys
import tomllib
import types
from pathlib import Path
from typing import Any


EXPECTED_SOURCE_FILES = 276
EXPECTED_PYTHON_MODULES = 274
EXPECTED_REGISTERED_ARTIFACTS = 439


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plugins", type=Path, help="Generated iLEAPP plugin folder")
    parser.add_argument(
        "--import-smoke",
        action="store_true",
        help="Import every plugin; requires the runtime dependencies to be installed",
    )
    return parser.parse_args()


def install_api_stub() -> None:
    module = types.ModuleType("cultivator_api")
    module.log = lambda level, message: None
    module.read_bytes = lambda path, max_bytes=None: Path(path).read_bytes()[:max_bytes]
    module.read_text = lambda path, max_bytes=None, encoding="utf-8", errors="replace": Path(path).read_text(
        encoding=encoding,
        errors=errors,
    )[:max_bytes]
    module.read_lines = lambda path, encoding="utf-8", errors="replace": Path(path).read_text(
        encoding=encoding,
        errors=errors,
    ).splitlines(keepends=True)
    module.create_table_artifact = lambda name, category, headers, **fields: {
        "kind": "custom_table",
        "name": name,
        "category": category,
        "headers": headers,
        "table": {"rows": []},
        **fields,
    }
    module.add_table_row = lambda table, values=None, **fields: table["table"]["rows"].append(
        {**(values or {}), **fields}
    )
    module.create_group = lambda label, id=None: {"label": label, "id": id or label}
    module.add_artifact = lambda artifact, file_path=None, group=None: None
    sys.modules["cultivator_api"] = module


def validate_manifest(path: Path) -> dict[str, Any]:
    with path.open("rb") as file:
        manifest = tomllib.load(file)
    required = {
        "id",
        "name",
        "author",
        "version",
        "description",
        "type",
        "target",
        "mode",
        "path_glob",
        "entry",
        "function",
    }
    missing = sorted(required - manifest.keys())
    if missing:
        raise ValueError(f"{path}: missing manifest fields {missing}")
    if manifest["target"] != "ios" or manifest["mode"] != "path_glob":
        raise ValueError(f"{path}: expected an iOS path_glob plugin")
    if not isinstance(manifest["path_glob"], list) or not manifest["path_glob"]:
        raise ValueError(f"{path}: path_glob must contain at least one pattern")
    return manifest


def validate_source(path: Path) -> None:
    source = path.read_text(encoding="utf-8")
    compile(source, str(path), "exec")
    tree = ast.parse(source, filename=str(path))
    functions = {node.name for node in tree.body if isinstance(node, ast.FunctionDef)}
    if "run" not in functions:
        raise ValueError(f"{path}: missing native run(context) entrypoint")
    if "import cultivator_api" not in source:
        raise ValueError(f"{path}: does not import cultivator_api")
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            imported_modules = [alias.name for alias in node.names]
        elif isinstance(node, ast.ImportFrom):
            imported_modules = [node.module or ""]
        else:
            continue
        if any(module == "scripts" or module.startswith("scripts.") for module in imported_modules):
            raise ValueError(f"{path}: still imports the iLEAPP scripts package")


def import_plugin(path: Path, plugin_id: str) -> None:
    module_name = "validation_" + re.sub(r"\W+", "_", plugin_id)
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Unable to construct an import spec for {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if not callable(getattr(module, "run", None)):
        raise ImportError(f"{path}: imported module has no callable run")


def validate(plugins: Path, import_smoke: bool) -> dict[str, int]:
    coverage_path = plugins / "coverage.json"
    coverage = json.loads(coverage_path.read_text(encoding="utf-8"))
    if coverage["sourceFileCount"] != EXPECTED_SOURCE_FILES:
        raise ValueError("Coverage does not account for all 276 source files")
    if coverage["pythonModuleCount"] != EXPECTED_PYTHON_MODULES:
        raise ValueError("Coverage does not account for all 274 Python modules")
    if coverage["registeredArtifactCount"] != EXPECTED_REGISTERED_ARTIFACTS:
        raise ValueError("Coverage does not account for all 439 registered artifacts")
    if len(coverage["modules"]) != EXPECTED_PYTHON_MODULES:
        raise ValueError("Coverage module mapping is incomplete")
    if {item["sourceFile"] for item in coverage["supportFiles"]} != {
        "NotificationParams.txt",
        "script.txt",
    }:
        raise ValueError("Coverage support-file mapping is incomplete")

    plugin_directories = sorted(
        path.parent for path in plugins.glob("*/plugin.toml") if path.parent.name != "_shared"
    )
    if len(plugin_directories) != EXPECTED_PYTHON_MODULES:
        raise ValueError(
            f"Expected {EXPECTED_PYTHON_MODULES} plugin directories, found {len(plugin_directories)}"
        )

    manifest_ids: set[str] = set()
    import_errors = []
    if import_smoke:
        install_api_stub()
        sys.path.insert(0, str(plugins / "_shared"))
    for directory in plugin_directories:
        manifest = validate_manifest(directory / "plugin.toml")
        if manifest["id"] in manifest_ids:
            raise ValueError(f"Duplicate plugin id: {manifest['id']}")
        manifest_ids.add(manifest["id"])
        source_path = directory / manifest["entry"]
        validate_source(source_path)
        if import_smoke:
            try:
                import_plugin(source_path, manifest["id"])
            except Exception as error:
                import_errors.append(f"{manifest['id']}: {type(error).__name__}: {error}")

    for support_source in (plugins / "_shared").rglob("*.py"):
        compile(support_source.read_text(encoding="utf-8"), str(support_source), "exec")

    if import_errors:
        raise RuntimeError("Plugin import failures:\n" + "\n".join(import_errors))

    mapped_directories = {module["pluginDirectory"] for module in coverage["modules"]}
    actual_directories = {directory.name for directory in plugin_directories}
    if mapped_directories != actual_directories:
        raise ValueError("Coverage plugin directories do not match generated directories")

    return {
        "sourceFiles": coverage["sourceFileCount"],
        "pythonModules": len(plugin_directories),
        "registeredArtifacts": coverage["registeredArtifactCount"],
        "supportFiles": len(coverage["supportFiles"]),
    }


def main() -> int:
    arguments = parse_arguments()
    result = validate(arguments.plugins.resolve(), arguments.import_smoke)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
