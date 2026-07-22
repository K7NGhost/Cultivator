"""Small shared helpers used across converted artifact parsers."""

from __future__ import annotations

import codecs
from typing import Any

from .io_helpers import current_function_name
from .output import collect_table


class iOS:
    _version = None

    @staticmethod
    def get_version():
        return iOS._version

    @staticmethod
    def set_version(os_version):
        if iOS._version is None:
            iOS._version = os_version


def device_info(category: str, label: str, value: Any, source_file: str = "") -> None:
    collect_table(
        "Device Information",
        ("Category", "Label", "Value", "Source File", "Artifact"),
        ((category, label, value, source_file, current_function_name()),),
        source_file,
        category="Device Information",
    )


def strings(data: bytes):
    cleansed = "".join(chr(byte) if 0x20 <= byte < 0x7F else "\0" for byte in data)
    return filter(lambda value: len(value) >= 4, cleansed.split("\0"))


def utf8_in_extended_ascii(input_string: str, *, raise_on_unexpected: bool = False):
    """Decode UTF-8 bytes that were incorrectly stored as extended ASCII characters."""
    try:
        raw = bytes(ord(character) for character in input_string)
        decoded = raw.decode("utf-8")
        return decoded != input_string, decoded
    except (UnicodeDecodeError, ValueError) as error:
        if raise_on_unexpected:
            raise ValueError("Unexpected data while repairing mis-encoded UTF-8") from error
        return False, input_string
