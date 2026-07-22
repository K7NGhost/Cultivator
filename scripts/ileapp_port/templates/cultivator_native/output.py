"""Native Cultivator table collection for converted iLEAPP parsers."""

from __future__ import annotations

import base64
import hashlib
import json
import re
from dataclasses import dataclass, field
from datetime import date, datetime
from pathlib import Path
from typing import Any, Iterable

import cultivator_api


@dataclass
class ExecutionMetadata:
    """Metadata for the artifact parser that is currently executing."""

    artifact_id: str
    category: str
    description: str
    icon: str | None
    module_name: str
    name: str
    source_paths: list[str]


@dataclass
class PendingTable:
    """Rows collected before one Cultivator custom-table artifact is emitted."""

    name: str
    category: str
    headers: list[Any]
    rows: list[Any] = field(default_factory=list)
    description: str = ""
    icon: str | None = None
    source_paths: list[str] = field(default_factory=list)


_current_metadata: ExecutionMetadata | None = None
_pending_tables: dict[tuple[str, tuple[str, ...]], PendingTable] = {}
_lava_tables: dict[str, tuple[str, list[Any]]] = {}


def begin_execution(metadata: ExecutionMetadata) -> None:
    global _current_metadata
    _current_metadata = metadata


def end_execution() -> None:
    global _current_metadata
    _current_metadata = None


def clear_tables() -> None:
    _pending_tables.clear()
    _lava_tables.clear()


def _header_label(header: Any) -> str:
    if isinstance(header, (list, tuple)) and header:
        return str(header[0])
    if isinstance(header, dict):
        return str(header.get("label") or header.get("key") or "Column")
    return str(header)


def _header_key(label: str, existing: set[str]) -> str:
    base = re.sub(r"[^0-9A-Za-z]+", "_", label.strip()).strip("_").lower()
    base = base or "column"
    key = base
    suffix = 2
    while key in existing:
        key = f"{base}_{suffix}"
        suffix += 1
    existing.add(key)
    return key


def _normalize_headers(headers: Iterable[Any]) -> tuple[list[dict[str, str]], list[str]]:
    normalized = []
    keys = []
    existing: set[str] = set()
    for header in headers:
        label = _header_label(header)
        key = _header_key(label, existing)
        normalized.append({"key": key, "label": label})
        keys.append(key)
    return normalized, keys


def _safe_bytes(value: bytes) -> str:
    if len(value) <= 256:
        return "base64:" + base64.b64encode(value).decode("ascii")
    digest = hashlib.sha256(value).hexdigest()
    return f"<{len(value)} bytes; sha256={digest}>"


def json_safe(value: Any) -> Any:
    """Convert parser values into data accepted by Cultivator's JSON boundary."""
    if value is None:
        return ""
    if isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, bytes):
        return _safe_bytes(value)
    if isinstance(value, (datetime, date)):
        return value.isoformat()
    if isinstance(value, Path):
        return str(value)
    if isinstance(value, dict):
        return {str(key): json_safe(item) for key, item in value.items()}
    if isinstance(value, (list, tuple, set, frozenset)):
        return [json_safe(item) for item in value]
    try:
        return json.loads(json.dumps(value, default=str))
    except (TypeError, ValueError):
        return str(value)


def _plain_rows(rows: Any) -> list[Any]:
    if rows is None:
        return []
    if isinstance(rows, tuple) and len(rows) == 2:
        first, second = rows
        if isinstance(first, (list, tuple)) and isinstance(second, (list, tuple)):
            return list(first)
    if isinstance(rows, list):
        return rows
    if isinstance(rows, tuple):
        return list(rows)
    try:
        return list(rows)
    except TypeError:
        return [rows]


def collect_table(
    name: str,
    headers: Iterable[Any],
    rows: Any,
    source_path: Any = None,
    *,
    category: str | None = None,
    description: str | None = None,
    icon: str | None = None,
) -> None:
    """Collect a parser result for one native Cultivator table artifact."""
    row_list = _plain_rows(rows)
    if not row_list:
        return

    header_list = list(headers)
    labels = tuple(_header_label(header) for header in header_list)
    key = (str(name), labels)
    metadata = _current_metadata
    sources = []
    if isinstance(source_path, (list, tuple, set)):
        sources.extend(str(path) for path in source_path if path)
    elif source_path:
        sources.append(str(source_path))
    elif metadata:
        sources.extend(metadata.source_paths)

    table = _pending_tables.get(key)
    if table is None:
        table = PendingTable(
            name=str(name),
            category=str(category or (metadata.category if metadata else "Other")),
            headers=header_list,
            description=str(description or (metadata.description if metadata else "")),
            icon=icon or (metadata.icon if metadata else None),
        )
        _pending_tables[key] = table

    table.rows.extend(row_list)
    for source in sources:
        if source not in table.source_paths:
            table.source_paths.append(source)


def flush_tables() -> int:
    """Emit every collected table through cultivator_api and return the count."""
    emitted = 0
    for table in _pending_tables.values():
        headers, keys = _normalize_headers(table.headers)
        if not headers:
            continue

        source_paths = table.source_paths
        source = {
            "filePath": source_paths[0] if source_paths else "",
            "parser": "iLEAPP native Cultivator port",
        }
        artifact = cultivator_api.create_table_artifact(
            name=table.name,
            category=table.category,
            headers=headers,
            label=table.name,
            icon=table.icon,
            description=table.description,
            source=source,
            raw={"sourcePaths": source_paths},
            deduplication={"mode": "group", "identityFields": keys},
        )

        for row in table.rows:
            if isinstance(row, dict):
                values = {
                    key: json_safe(row.get(label, row.get(key, "")))
                    for key, label in zip(keys, (item["label"] for item in headers), strict=True)
                }
            else:
                try:
                    values_list = list(row)
                except TypeError:
                    values_list = [row]
                values = {
                    key: json_safe(values_list[index] if index < len(values_list) else "")
                    for index, key in enumerate(keys)
                }
            cultivator_api.add_table_row(artifact, values)

        group_label = _current_group_label(table.name)
        group = cultivator_api.create_group(group_label)
        cultivator_api.add_artifact(
            artifact,
            file_path=source_paths[0] if source_paths else None,
            group=group,
        )
        emitted += 1

    clear_tables()
    return emitted


def _current_group_label(fallback: str) -> str:
    metadata = _current_metadata
    return metadata.module_name if metadata else fallback


class CultivatorTableReport:
    """Collect old report-table calls as native Cultivator table artifacts."""

    def __init__(self, artifact_name: str, artifact_category: str = "") -> None:
        self.artifact_name = str(artifact_name)
        self.artifact_category = str(artifact_category)
        self.description = ""
        self.section_heading = ""

    def start_artifact_report(
        self,
        report_folder: str,
        artifact_file_name: str,
        artifact_description: str = "",
    ) -> None:
        del report_folder, artifact_file_name
        self.description = str(artifact_description)

    def add_script(self, script: str = "") -> None:
        del script

    def write_artifact_data_table(
        self,
        data_headers: Iterable[Any],
        data_list: Any,
        source_path: Any,
        **options: Any,
    ) -> None:
        del options
        table_name = self.section_heading or self.artifact_name
        collect_table(
            table_name,
            data_headers,
            data_list,
            source_path,
            category=self.artifact_category or None,
            description=self.description or None,
        )
        self.section_heading = ""

    def add_section_heading(self, heading: str, size: str = "h2") -> None:
        del size
        self.section_heading = str(heading)

    def write_minor_header(self, heading: str, heading_tag: str = "") -> None:
        del heading, heading_tag

    def write_lead_text(self, text: str) -> None:
        del text

    def write_raw_html(self, code: str) -> None:
        del code

    def end_artifact_report(self) -> None:
        return None


def lava_process_artifact(
    category: str,
    module_name: str,
    artifact_name: str,
    data: Iterable[Any],
    record_count: int | None = None,
    data_views: Any = None,
) -> tuple[str, list[Any], dict[str, Any]]:
    del module_name, record_count, data_views
    table_name = str(artifact_name)
    _lava_tables[table_name] = (str(category), list(data))
    return table_name, [], {}


def lava_insert_sqlite_data(
    table_name: str,
    data: Any,
    object_columns: Any,
    headers: Iterable[Any],
    column_map: Any,
) -> None:
    del object_columns, column_map
    category, registered_headers = _lava_tables.get(
        str(table_name),
        (_current_metadata.category if _current_metadata else "Other", list(headers)),
    )
    collect_table(str(table_name), registered_headers or headers, data, category=category)


def tsv(*args: Any, **kwargs: Any) -> None:
    """TSV output is represented by the native table and is intentionally not duplicated."""
    del args, kwargs


def timeline(*args: Any, **kwargs: Any) -> None:
    """Legacy timeline export is not duplicated beside the native table."""
    del args, kwargs


def kmlgen(*args: Any, **kwargs: Any) -> None:
    """Legacy KML export is not duplicated beside the native table."""
    del args, kwargs
