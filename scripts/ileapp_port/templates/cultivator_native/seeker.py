"""Datasource search adapter for parsers that need related files."""

from __future__ import annotations

import fnmatch
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


@dataclass(frozen=True)
class FileInfo:
    source_path: str
    creation_date: float
    modification_date: float


class FileInfoIndex(dict[str, FileInfo]):
    def __missing__(self, path: str) -> FileInfo:
        try:
            stat = Path(path).stat()
            info = FileInfo(str(path), stat.st_ctime, stat.st_mtime)
        except OSError:
            info = FileInfo(str(path), 0, 0)
        self[path] = info
        return info

    def get(self, path: str, default=None):
        if not path:
            return default
        return self[path]


def path_matches(path: str, pattern: str) -> bool:
    normalized_path = path.replace("\\", "/").casefold()
    normalized_pattern = pattern.replace("\\", "/").casefold()
    if normalized_pattern.startswith("*/"):
        normalized_pattern = "**/" + normalized_pattern[2:]
    return fnmatch.fnmatchcase(normalized_path, normalized_pattern) or fnmatch.fnmatchcase(
        Path(normalized_path).name,
        normalized_pattern,
    )


class CultivatorSeeker:
    """Provide the small file-search surface used by the converted parsers."""

    def __init__(self, datasource_paths: Iterable[str], matched_files: Iterable[str]) -> None:
        self.datasource_paths = [str(path) for path in datasource_paths]
        self.data_folder = self.datasource_paths[0] if self.datasource_paths else ""
        self.file_infos = FileInfoIndex()
        self._matched_files = list(dict.fromkeys(str(path) for path in matched_files))
        self._all_files: list[str] | None = None
        for path in self._matched_files:
            self.file_infos[path]

    def _enumerate_files(self) -> list[str]:
        if self._all_files is not None:
            return self._all_files
        files = list(self._matched_files)
        seen = {os.path.normcase(os.path.abspath(path)) for path in files}
        for source in self.datasource_paths:
            source_path = Path(source)
            if source_path.is_file():
                candidates = [source_path]
            elif source_path.is_dir():
                candidates = (path for path in source_path.rglob("*") if path.is_file())
            else:
                continue
            for candidate in candidates:
                path = str(candidate)
                key = os.path.normcase(os.path.abspath(path))
                if key in seen:
                    continue
                seen.add(key)
                files.append(path)
                self.file_infos[path]
        self._all_files = files
        return files

    def search(
        self,
        pattern: str,
        return_on_first_hit: bool = False,
        force: bool = False,
    ):
        del force
        matches = [path for path in self._enumerate_files() if path_matches(path, pattern)]
        if return_on_first_hit:
            return matches[0] if matches else None
        return matches
