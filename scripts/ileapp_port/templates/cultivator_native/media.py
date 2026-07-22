"""Media extraction helpers for native Cultivator parsers."""

from __future__ import annotations

import hashlib
import mimetypes
import shutil
from pathlib import Path
from typing import Any, Iterable

from .io_helpers import logfunc, sanitize_file_name
from .seeker import path_matches


class MediaInfo(tuple):
    FIELDS = (
        "media_ref_id",
        "media_item_id",
        "module_name",
        "artifact_name",
        "name",
        "media_path",
        "source_path",
        "extraction_path",
        "type",
        "metadata",
        "created_at",
        "updated_at",
    )

    def __new__(cls, values):
        return super().__new__(cls, values)

    def __getitem__(self, item):
        if isinstance(item, str):
            item = self.FIELDS.index(item)
        return super().__getitem__(item)


_media_items: dict[str, MediaInfo] = {}


def _artifact_name(artifact_info: Any) -> str:
    return str(getattr(artifact_info, "function", None) or "artifact")


def _copy_media(source: Path, report_folder: str, name: str, artifact_name: str) -> str:
    digest = hashlib.sha1(str(source).encode("utf-8"), usedforsecurity=False).hexdigest()
    output_root = Path(report_folder) / "iLEAPP Media"
    output_root.mkdir(parents=True, exist_ok=True)
    safe_name = sanitize_file_name(name or source.stem)
    destination = output_root / f"{digest}-{safe_name}{source.suffix}"
    if source.resolve() != destination.resolve() and not destination.exists():
        shutil.copy2(source, destination)
    stat = source.stat()
    media_type = mimetypes.guess_type(source.name)[0] or "application/octet-stream"
    _media_items[digest] = MediaInfo(
        (
            digest,
            digest,
            "cultivator",
            artifact_name,
            name,
            str(destination),
            str(source),
            str(destination),
            media_type,
            "",
            stat.st_ctime,
            stat.st_mtime,
        )
    )
    return digest


def check_in_media(
    artifact_info: Any,
    report_folder: str,
    seeker: Any,
    files_found: Iterable[str],
    file_path: str,
    name: str = "",
    converted_file_path: Any = False,
) -> str | None:
    del seeker
    source = next(
        (Path(path) for path in files_found if path_matches(str(path), str(file_path))),
        None,
    )
    if source is None and Path(str(file_path)).is_file():
        source = Path(str(file_path))
    if source is None:
        logfunc(f"No matching media file found for '{file_path}'")
        return None
    if converted_file_path and Path(str(converted_file_path)).is_file():
        source = Path(str(converted_file_path))
    try:
        return _copy_media(source, report_folder, name, _artifact_name(artifact_info))
    except OSError as error:
        logfunc(f"Unable to extract media '{source}': {error}")
        return None


def check_in_embedded_media(
    artifact_info: Any,
    report_folder: str,
    seeker: Any,
    source_file: str,
    data: bytes,
    name: str = "",
) -> str | None:
    del seeker
    if not data:
        return None
    digest = hashlib.sha1(data, usedforsecurity=False).hexdigest()
    output_root = Path(report_folder) / "iLEAPP Media"
    output_root.mkdir(parents=True, exist_ok=True)
    extension = mimetypes.guess_extension(mimetypes.guess_type(name)[0] or "") or ".bin"
    destination = output_root / f"{digest}-{sanitize_file_name(name or 'embedded')}{extension}"
    try:
        if not destination.exists():
            destination.write_bytes(data)
        stat = destination.stat()
    except OSError as error:
        logfunc(f"Unable to extract embedded media from '{source_file}': {error}")
        return None
    media_type = mimetypes.guess_type(destination.name)[0] or "application/octet-stream"
    _media_items[digest] = MediaInfo(
        (
            digest,
            digest,
            "cultivator",
            _artifact_name(artifact_info),
            name,
            str(destination),
            str(source_file),
            str(destination),
            media_type,
            "",
            stat.st_ctime,
            stat.st_mtime,
        )
    )
    return digest


def lava_get_full_media_info(media_ref_id: str | None) -> MediaInfo | None:
    if not media_ref_id:
        return None
    return _media_items.get(str(media_ref_id))


def media_to_html(media_path: str, files_found: Iterable[str], report_folder: str) -> str:
    del report_folder
    return next(
        (str(path) for path in files_found if path_matches(str(path), str(media_path))),
        str(media_path),
    )


def generate_thumbnail(im_directory: str, im_filename: str, seeker: Any, report_folder: str) -> str:
    pattern = f"**/Media/PhotoData/Thumbnails/**/{im_directory}/{im_filename}/**.JPG"
    match = seeker.search(pattern, return_on_first_hit=True)
    if not match:
        return ""
    reference = check_in_media(None, report_folder, seeker, [match], match, im_filename)
    info = lava_get_full_media_info(reference)
    return info["extraction_path"] if info else ""
