"""Inventory iLEAPP artifact modules without importing their dependencies."""

from __future__ import annotations

import argparse
import ast
import json
from pathlib import Path
from typing import Any


METADATA_NAMES = ("__artifacts_v2__", "__artifacts__")


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build a static inventory of iLEAPP scripts/artifacts.",
    )
    parser.add_argument("source", type=Path, help="Path to scripts/artifacts")
    parser.add_argument("--output", type=Path, help="Optional JSON output path")
    return parser.parse_args()


def literal(node: ast.AST | None, default: Any = None) -> Any:
    if node is None:
        return default

    try:
        return ast.literal_eval(node)
    except (TypeError, ValueError):
        return default


def dictionary_items(node: ast.AST) -> list[tuple[ast.AST, ast.AST]]:
    if not isinstance(node, ast.Dict):
        return []

    return [
        (key, value)
        for key, value in zip(node.keys, node.values, strict=True)
        if key is not None
    ]


def dictionary_value(node: ast.AST, key_name: str) -> ast.AST | None:
    for key, value in dictionary_items(node):
        if literal(key) == key_name:
            return value

    return None


def normalize_paths(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]

    if isinstance(value, (list, tuple)):
        return [item for item in value if isinstance(item, str)]

    return []


def callable_name(node: ast.AST | None) -> str | None:
    if isinstance(node, ast.Name):
        return node.id

    if isinstance(node, ast.Attribute):
        return node.attr

    return None


def parse_v2_artifacts(node: ast.AST) -> list[dict[str, Any]]:
    artifacts = []

    for key_node, value_node in dictionary_items(node):
        artifact_id = literal(key_node)
        if not isinstance(artifact_id, str) or not isinstance(value_node, ast.Dict):
            continue

        name = literal(dictionary_value(value_node, "name"), artifact_id)
        category = literal(dictionary_value(value_node, "category"), "Other")
        description = literal(dictionary_value(value_node, "description"), "")
        author = literal(dictionary_value(value_node, "author"), "iLEAPP contributors")
        icon = literal(dictionary_value(value_node, "artifact_icon"))
        paths = normalize_paths(literal(dictionary_value(value_node, "paths"), ()))
        function_name = literal(dictionary_value(value_node, "function"))

        artifacts.append(
            {
                "id": artifact_id,
                "name": name if isinstance(name, str) else artifact_id,
                "category": category if isinstance(category, str) else "Other",
                "description": description if isinstance(description, str) else "",
                "author": author if isinstance(author, str) else "iLEAPP contributors",
                "icon": icon if isinstance(icon, str) else None,
                "paths": paths,
                "function": function_name if isinstance(function_name, str) else artifact_id,
                "metadataVersion": 2,
            }
        )

    return artifacts


def parse_v1_artifacts(node: ast.AST) -> list[dict[str, Any]]:
    artifacts = []

    for key_node, value_node in dictionary_items(node):
        artifact_id = literal(key_node)
        if not isinstance(artifact_id, str):
            continue

        if not isinstance(value_node, (ast.Tuple, ast.List)) or len(value_node.elts) < 3:
            continue

        category = literal(value_node.elts[0], "Other")
        paths = normalize_paths(literal(value_node.elts[1], ()))
        function_name = callable_name(value_node.elts[2])

        artifacts.append(
            {
                "id": artifact_id,
                "name": artifact_id,
                "category": category if isinstance(category, str) else "Other",
                "description": "",
                "author": "iLEAPP contributors",
                "icon": None,
                "paths": paths,
                "function": function_name,
                "metadataVersion": 1,
            }
        )

    return artifacts


def imported_roots(tree: ast.Module) -> list[str]:
    roots = set()

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            roots.update(alias.name.split(".", 1)[0] for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.module:
            roots.add(node.module.split(".", 1)[0])

    return sorted(roots)


def imported_symbols(tree: ast.Module) -> dict[str, list[str]]:
    """Return imported module names and their referenced symbols."""
    imports: dict[str, set[str]] = {}

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                imports.setdefault(alias.name, set()).add("*")
        elif isinstance(node, ast.ImportFrom) and node.module:
            imports.setdefault(node.module, set()).update(alias.name for alias in node.names)

    return {
        module: sorted(symbols)
        for module, symbols in sorted(imports.items())
    }


def inventory_module(path: Path) -> dict[str, Any]:
    source = path.read_text(encoding="utf-8-sig", errors="replace")

    try:
        tree = ast.parse(source, filename=str(path))
    except SyntaxError as error:
        return {
            "file": path.name,
            "artifacts": [],
            "imports": [],
            "importedSymbols": {},
            "functions": [],
            "parseError": f"{error.msg} at line {error.lineno}",
        }

    metadata = {}
    decorated_functions = []
    function_names = []

    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            function_names.append(node.name)
            if any(callable_name(decorator) == "artifact_processor" for decorator in node.decorator_list):
                decorated_functions.append(node.name)

        if not isinstance(node, (ast.Assign, ast.AnnAssign)):
            continue

        targets = node.targets if isinstance(node, ast.Assign) else [node.target]
        value = node.value

        for target in targets:
            if isinstance(target, ast.Name) and target.id in METADATA_NAMES:
                metadata[target.id] = value

    artifacts = []
    if "__artifacts_v2__" in metadata:
        artifacts = parse_v2_artifacts(metadata["__artifacts_v2__"])
    elif "__artifacts__" in metadata:
        artifacts = parse_v1_artifacts(metadata["__artifacts__"])

    return {
        "file": path.name,
        "artifacts": artifacts,
        "imports": imported_roots(tree),
        "importedSymbols": imported_symbols(tree),
        "functions": sorted(function_names),
        "decoratedFunctions": decorated_functions,
        "parseError": None,
    }


def build_inventory(source: Path) -> dict[str, Any]:
    files = sorted(path for path in source.iterdir() if path.is_file())
    python_files = [path for path in files if path.suffix.casefold() == ".py"]
    support_files = [path.name for path in files if path not in python_files]
    modules = [inventory_module(path) for path in python_files]
    parse_errors = [module for module in modules if module["parseError"]]
    modules_without_metadata = [
        module["file"] for module in modules if not module["artifacts"]
    ]

    return {
        "source": str(source.resolve()),
        "sourceFileCount": len(files),
        "pythonModuleCount": len(python_files),
        "supportFiles": support_files,
        "registeredArtifactCount": sum(
            len(module["artifacts"]) for module in modules
        ),
        "modulesWithoutMetadata": modules_without_metadata,
        "parseErrors": [
            {"file": module["file"], "error": module["parseError"]}
            for module in parse_errors
        ],
        "modules": modules,
    }


def main() -> int:
    arguments = parse_arguments()
    source = arguments.source.resolve()

    if not source.is_dir():
        raise SystemExit(f"Artifact source directory does not exist: {source}")

    inventory = build_inventory(source)
    serialized = json.dumps(inventory, indent=2, sort_keys=True)

    if arguments.output:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(serialized + "\n", encoding="utf-8")

    print(
        json.dumps(
            {
                "sourceFileCount": inventory["sourceFileCount"],
                "pythonModuleCount": inventory["pythonModuleCount"],
                "supportFiles": inventory["supportFiles"],
                "registeredArtifactCount": inventory["registeredArtifactCount"],
                "modulesWithoutMetadata": inventory["modulesWithoutMetadata"],
                "parseErrors": inventory["parseErrors"],
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
