"""Execution entrypoint for rewritten native Cultivator artifact modules."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any

import cultivator_api

from .output import (
    ExecutionMetadata,
    begin_execution,
    clear_tables,
    collect_table,
    end_execution,
    flush_tables,
)
from .seeker import CultivatorSeeker, path_matches


def _matched_files(
    context: dict[str, Any],
    artifact_specs: list[dict[str, Any]],
) -> list[str]:
    plugin = context.get("plugin", {})
    matched = plugin.get("matched_files") or plugin.get("matchedFiles")
    if isinstance(matched, list) and matched:
        return list(dict.fromkeys(str(path) for path in matched))

    patterns = [
        str(pattern)
        for spec in artifact_specs
        for pattern in spec.get("paths", [])
    ]
    current = context.get("file", {}).get("path")
    discovered = [str(current)] if current else []
    seen = {str(current).casefold()} if current else set()
    for datasource_path in context.get("datasource", {}).get("paths", []):
        root = Path(datasource_path)
        if root.is_file():
            candidates = (root,)
        elif root.is_dir():
            candidates = (path for path in root.rglob("*") if path.is_file())
        else:
            continue
        for candidate in candidates:
            path = str(candidate)
            if path.casefold() in seen or not any(path_matches(path, pattern) for pattern in patterns):
                continue
            seen.add(path.casefold())
            discovered.append(path)
    cultivator_api.log(
        "warning",
        "Cultivator did not provide plugin.matched_files; the plugin scanned datasource paths as a fallback.",
    )
    return discovered


def _artifact_files(all_files: list[str], paths: list[str]) -> list[str]:
    return [
        file_path
        for file_path in all_files
        if any(path_matches(file_path, pattern) for pattern in paths)
    ]


def _metadata(spec: dict[str, Any], module_name: str, source_paths: list[str]) -> ExecutionMetadata:
    return ExecutionMetadata(
        artifact_id=str(spec["id"]),
        category=str(spec.get("category") or "Other"),
        description=str(spec.get("description") or ""),
        icon=str(spec["icon"]) if spec.get("icon") else None,
        module_name=module_name,
        name=str(spec.get("name") or spec["id"]),
        source_paths=source_paths,
    )


def _collect_return_value(spec: dict[str, Any], result: Any) -> None:
    if not isinstance(result, tuple) or len(result) != 3:
        if result is not None:
            cultivator_api.log(
                "warning",
                f"Parser '{spec['function']}' returned an unsupported result shape.",
            )
        return
    headers, rows, source_path = result
    collect_table(
        str(spec.get("name") or spec["id"]),
        headers,
        rows,
        source_path,
        category=str(spec.get("category") or "Other"),
        description=str(spec.get("description") or ""),
        icon=str(spec["icon"]) if spec.get("icon") else None,
    )


def run_module(
    context: dict[str, Any],
    module_globals: dict[str, Any],
    artifact_specs: list[dict[str, Any]],
) -> None:
    """Execute every registered parser in one rewritten source module exactly once."""
    all_files = _matched_files(context, artifact_specs)
    module_name = str(context.get("plugin", {}).get("name") or "iLEAPP")
    report_folder = Path(context["case"]["artifacts_path"]) / "iLEAPP"
    report_folder.mkdir(parents=True, exist_ok=True)
    seeker = CultivatorSeeker(context.get("datasource", {}).get("paths", []), all_files)
    timezone_name = str(context.get("options", {}).get("timezone") or "UTC")
    clear_tables()

    for spec in artifact_specs:
        source_paths = _artifact_files(all_files, list(spec.get("paths") or []))
        if not source_paths:
            continue
        metadata = _metadata(spec, module_name, source_paths)
        begin_execution(metadata)
        parser = module_globals.get(str(spec["function"]))
        if not callable(parser):
            cultivator_api.log(
                "error",
                f"Native parser function '{spec['function']}' was not found for '{spec['name']}'.",
            )
            continue
        try:
            result = parser(source_paths, str(report_folder), seeker, False, timezone_name)
            _collect_return_value(spec, result)
        except Exception as error:
            cultivator_api.log(
                "error",
                f"Parser '{spec['name']}' failed: {type(error).__name__}: {error}",
            )

    try:
        emitted = flush_tables()
        cultivator_api.log("info", f"{module_name}: emitted {emitted} native artifact table(s).")
    finally:
        end_execution()
