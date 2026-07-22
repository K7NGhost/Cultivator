"""Timestamp conversions retained by native Cultivator artifact parsers."""

from __future__ import annotations

import math
from datetime import UTC, date, datetime, time, timedelta, timezone
from typing import Any

try:
    import pytz
except ImportError:
    pytz = None


COCOA_EPOCH_OFFSET = 978_307_200


def _unix_seconds(value: int | float) -> int:
    numeric = float(value)
    if numeric == 0:
        return 0
    while abs(numeric) >= 10_000_000_000:
        numeric /= 10
    return int(numeric)


def convert_unix_ts_to_utc(ts: Any) -> Any:
    if not ts:
        return ts
    return datetime.fromtimestamp(_unix_seconds(ts), tz=UTC)


def convert_unix_ts_to_str(ts: Any) -> Any:
    converted = convert_unix_ts_to_utc(ts)
    return converted.strftime("%Y-%m-%d %H:%M:%S") if converted else converted


def convert_human_ts_to_utc(ts: Any) -> Any:
    if not ts:
        return ts
    text = str(ts).split(".", 1)[0]
    return datetime.strptime(text, "%Y-%m-%d %H:%M:%S").replace(tzinfo=UTC)


def convert_cocoa_core_data_ts_to_utc(ts: Any) -> Any:
    if not ts:
        return ts
    return convert_unix_ts_to_utc(float(ts) + COCOA_EPOCH_OFFSET)


def convert_log_ts_to_utc(ts: Any) -> Any:
    if not ts:
        return ts
    try:
        return datetime.strptime(str(ts), "%b %d %Y %H:%M:%S").replace(tzinfo=UTC)
    except ValueError:
        return ts


def convert_local_to_utc(local_timestamp_str: str) -> datetime:
    value = datetime.strptime(local_timestamp_str, "%Y-%m-%d %H:%M:%S%z")
    return value.astimezone(UTC)


def convert_time_obj_to_utc(ts: Any) -> Any:
    return ts.replace(tzinfo=UTC) if ts else ts


def _timezone(timezone_name: str):
    if pytz is None:
        if timezone_name in {"UTC", "Etc/UTC", "GMT"}:
            return UTC
        raise RuntimeError("pytz is required for non-UTC timezone conversion")
    return pytz.timezone(timezone_name or "UTC")


def convert_utc_human_to_timezone(utc_time: Any, time_offset: str) -> Any:
    if not utc_time:
        return utc_time
    if isinstance(utc_time, str):
        utc_time = convert_ts_human_to_utc(utc_time)
    return utc_time.astimezone(_timezone(time_offset))


def convert_ts_int_to_timezone(value: Any, time_offset: str) -> Any:
    return convert_utc_human_to_timezone(convert_ts_int_to_utc(value), time_offset)


def webkit_timestampsconv(webkittime: Any) -> Any:
    if webkittime is None:
        return webkittime
    return datetime.fromtimestamp(float(webkittime) + COCOA_EPOCH_OFFSET, tz=UTC)


def convert_ts_human_to_utc(ts: Any) -> Any:
    if not ts:
        return ts
    if isinstance(ts, datetime):
        return ts.replace(tzinfo=ts.tzinfo or UTC)
    text = str(ts).split(".", 1)[0]
    return datetime.strptime(text, "%Y-%m-%d %H:%M:%S").replace(tzinfo=UTC)


def convert_ts_int_to_utc(ts: Any) -> Any:
    if ts is None or ts == "":
        return ts
    return datetime.fromtimestamp(float(ts), tz=UTC)


def convert_unix_ts_to_timezone(ts: Any, timezone_offset: str) -> Any:
    converted = convert_unix_ts_to_utc(ts)
    return convert_utc_human_to_timezone(converted, timezone_offset) if converted else converted


def convert_ts_human_to_timezone_offset(ts: Any, timezone_offset: str) -> Any:
    converted = convert_ts_human_to_utc(ts)
    return convert_utc_human_to_timezone(converted, timezone_offset) if converted else converted


def convert_plist_date_to_timezone_offset(plist_date: Any, timezone_offset: str) -> Any:
    converted = convert_plist_date_to_utc(plist_date)
    return convert_utc_human_to_timezone(converted, timezone_offset) if converted else converted


def convert_plist_date_to_utc(plist_date: Any) -> Any:
    if not plist_date:
        return plist_date
    if isinstance(plist_date, datetime):
        return plist_date.replace(tzinfo=plist_date.tzinfo or UTC)
    if isinstance(plist_date, date):
        return datetime.combine(plist_date, time.min, tzinfo=UTC)
    return plist_date


def get_birthdate(value: int | float) -> str:
    cocoa_epoch = datetime(2001, 1, 1, tzinfo=UTC)
    result = cocoa_epoch + timedelta(seconds=value)
    return result.strftime("%d %B %Y") if result.year != 1604 else result.strftime("%d %B")


def convert_bytes_to_unit(size: Any) -> Any:
    if not size:
        return size
    numeric = float(size)
    for unit in ("bytes", "KB", "MB", "GB"):
        if numeric < 1024:
            return f"{numeric:3.1f} {unit}"
        numeric /= 1024
    return numeric
