"""File, plist, and SQLite helpers used by native Cultivator parsers."""

from __future__ import annotations

import inspect
import os
import plistlib
import re
import sqlite3
import xml.parsers.expat
from pathlib import Path
from typing import Any, Iterable

import cultivator_api

try:
    import nska_deserialize
except ImportError:  # A normal plist can still be parsed without this optional package.
    nska_deserialize = None


def artifact_processor(function):
    """Mark a parser function without wrapping its native return value."""
    function.__cultivator_artifact_parser__ = True
    return function


def logfunc(message: Any = "") -> None:
    cultivator_api.log("info", str(message))


def logdevinfo(message: Any = "") -> None:
    cultivator_api.log("info", str(message))


def is_platform_windows() -> bool:
    return os.name == "nt"


def sanitize_file_name(filename: str, replacement_char: str = "_") -> str:
    return re.sub(r"[\\/*?:\"<>|'\r\n]", replacement_char, filename)


def get_next_unused_name(path: str) -> str:
    candidate = Path(path)
    if not candidate.exists():
        return str(candidate)
    for suffix in range(1, 10_000):
        next_path = candidate.with_name(f"{candidate.stem}-{suffix:02}{candidate.suffix}")
        if not next_path.exists():
            return str(next_path)
    raise FileExistsError(f"No unused filename is available for {path}")


def get_file_path(files_found: Iterable[Any], filename: str, skip: Any = False) -> str | None:
    for file_found in files_found:
        path = str(file_found)
        if skip and str(skip) in path:
            continue
        if Path(path).match(filename) or Path(path).name == filename:
            return path
    return None


def get_txt_file_content(file_path: str | os.PathLike[str] | None) -> list[str]:
    if not file_path:
        return []
    try:
        return list(cultivator_api.read_lines(str(file_path)))
    except Exception as error:
        logfunc(f"Unable to read text file '{file_path}': {error}")
        return []


def _deserialize_keyed_archive(data: bytes, path: str | None = None) -> Any:
    if nska_deserialize is None:
        raise RuntimeError("nska_deserialize is required for NSKeyedArchive data")
    if path:
        return nska_deserialize.deserialize_plist(path)
    return nska_deserialize.deserialize_plist_from_string(data)


def get_plist_content(data: Any) -> Any:
    try:
        if isinstance(data, str):
            data = data.encode("utf-8")
        plist_content = plistlib.loads(data)
        if isinstance(plist_content, dict) and plist_content.get("$archiver") == "NSKeyedArchiver":
            return _deserialize_keyed_archive(data)
        return plist_content
    except Exception as error:
        logfunc(f"Unable to parse plist data: {error}")
        return {}


def get_plist_file_content(file_path: str | os.PathLike[str] | None) -> Any:
    if not file_path:
        return {}
    try:
        data = cultivator_api.read_bytes(str(file_path))
        plist_content = plistlib.loads(data)
        if isinstance(plist_content, dict) and plist_content.get("$archiver") == "NSKeyedArchiver":
            return _deserialize_keyed_archive(data, str(file_path))
        return plist_content
    except Exception as error:
        logfunc(f"Unable to parse plist file '{file_path}': {error}")
        return {}


def _sqlite_uri(path: str | os.PathLike[str]) -> str:
    resolved = Path(path).resolve()
    return resolved.as_uri() + "?mode=ro"


def open_sqlite_db_readonly(path: str | os.PathLike[str] | None) -> sqlite3.Connection | None:
    if not path:
        return None
    try:
        return sqlite3.connect(_sqlite_uri(path), uri=True)
    except sqlite3.Error as error:
        logfunc(f"Unable to open SQLite database '{path}': {error}")
        return None


def attach_sqlite_db_readonly(path: str | os.PathLike[str] | None, db_name: str) -> str:
    if not path:
        return ""
    escaped_uri = _sqlite_uri(path).replace("'", "''")
    safe_name = re.sub(r"[^0-9A-Za-z_]", "_", db_name)
    return f"ATTACH DATABASE '{escaped_uri}' AS {safe_name}"


def get_sqlite_db_records(
    path: str | os.PathLike[str] | None,
    query: str,
    attach_query: str | None = None,
) -> list[sqlite3.Row]:
    database = open_sqlite_db_readonly(path)
    if database is None:
        return []
    database.row_factory = sqlite3.Row
    try:
        cursor = database.cursor()
        if attach_query:
            cursor.execute(attach_query)
        cursor.execute(query)
        return cursor.fetchall()
    except sqlite3.Error as error:
        logfunc(f"SQLite query failed for '{path}': {error}")
        return []
    finally:
        database.close()


def get_sqlite_multiple_db_records(
    path_list: Iterable[str],
    query: str,
    data_headers: Iterable[Any],
) -> tuple[tuple[Any, ...], list[Any], str]:
    paths = list(path_list)
    headers = tuple(data_headers)
    multiple_sources = len(paths) > 1
    if multiple_sources:
        headers += ("Source Path",)
    rows = []
    for path in paths:
        for record in get_sqlite_db_records(path, query):
            row = tuple(record)
            rows.append(row + (path,) if multiple_sources else row)
    source_path = "file path in the table below" if multiple_sources else (paths[0] if paths else "")
    return headers, rows, source_path


def does_column_exist_in_db(path: str, table_name: str, col_name: str) -> bool:
    database = open_sqlite_db_readonly(path)
    if database is None:
        return False
    database.row_factory = sqlite3.Row
    try:
        rows = database.execute(f"PRAGMA table_info('{table_name}')").fetchall()
        return any(str(row["name"]).casefold() == col_name.casefold() for row in rows)
    except sqlite3.Error as error:
        logfunc(f"Unable to inspect column '{table_name}.{col_name}': {error}")
        return False
    finally:
        database.close()


def _sqlite_object_exists(path: str, name: str, object_type: str) -> bool:
    database = open_sqlite_db_readonly(path)
    if database is None:
        return False
    try:
        row = database.execute(
            "SELECT 1 FROM sqlite_master WHERE type = ? AND name = ? LIMIT 1",
            (object_type, name),
        ).fetchone()
        return row is not None
    except sqlite3.Error as error:
        logfunc(f"Unable to inspect SQLite {object_type} '{name}': {error}")
        return False
    finally:
        database.close()


def does_table_exist_in_db(path: str, table_name: str) -> bool:
    return _sqlite_object_exists(path, table_name, "table")


def does_view_exist_in_db(path: str, table_name: str) -> bool:
    return _sqlite_object_exists(path, table_name, "view")


def current_function_name() -> str:
    try:
        return inspect.stack()[2].function
    except (IndexError, AttributeError):
        return "unknown"
