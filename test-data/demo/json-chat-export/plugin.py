"""Parses JSON message, contact, and call-log exports into Cultivator artifacts.

Expected layout (any depth under the datasource):
    <device>/messages/*.json  [{"from", "to", "timestamp", "body", "service"?}]
    <device>/contacts/*.json  [{"name", "phones": [...], "emails": [...]}]
    <device>/calls/*.json     [{"timestamp", "direction", "number", "durationSeconds"}]

Phone numbers in messages and calls are resolved to names using the
contacts export that sits alongside them, so artifacts read
"Jane Miller (+1-555-0177)" instead of a bare number.
"""

import json
import os

_contacts_cache = {}


def _load_json(path):
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def _device_root(path):
    return os.path.dirname(os.path.dirname(path))


def _contacts_for(path):
    root = _device_root(path)
    if root in _contacts_cache:
        return _contacts_cache[root]

    names = {}
    contacts_dir = os.path.join(root, "contacts")
    if os.path.isdir(contacts_dir):
        for name in os.listdir(contacts_dir):
            if not name.lower().endswith(".json"):
                continue
            try:
                for contact in _load_json(os.path.join(contacts_dir, name)):
                    for phone in contact.get("phones", []):
                        names[phone] = contact.get("name", phone)
            except (OSError, ValueError):
                continue

    _contacts_cache[root] = names
    return names


def _display(number, names):
    name = names.get(number)
    return f"{name} ({number})" if name else number


def _parse_messages(path, records):
    names = _contacts_for(path)
    artifacts = []
    for record in records:
        sender = _display(record.get("from", ""), names)
        recipient = _display(record.get("to", ""), names)
        participants = sorted([record.get("from", ""), record.get("to", "")])
        sent_at = record.get("timestamp", "")
        artifacts.append({
            "kind": "message",
            "label": f"{sender} -> {recipient}",
            "conversationId": "|".join(participants),
            "sender": sender,
            "recipients": [recipient],
            "body": record.get("body", ""),
            "sentAt": sent_at,
            "service": record.get("service", "SMS"),
            "timestamps": [{"value": sent_at, "label": "Sent"}],
            "source": {"filePath": path},
        })
    return artifacts


def _parse_contacts(path, records):
    return [
        {
            "kind": "contact",
            "label": record.get("name", "Unknown"),
            "name": record.get("name", "Unknown"),
            "phones": record.get("phones", []),
            "emails": record.get("emails", []),
            "notes": "Device owner" if record.get("isDeviceOwner") else "",
            "source": {"filePath": path},
        }
        for record in records
    ]


def _parse_calls(path, records):
    names = _contacts_for(path)
    artifacts = []
    for record in records:
        number = record.get("number", "")
        started_at = record.get("timestamp", "")
        artifacts.append({
            "kind": "call",
            "label": f"{record.get('direction', 'unknown')} call {_display(number, names)}",
            "direction": record.get("direction", "unknown"),
            "phone": number,
            "contactName": names.get(number, ""),
            "startedAt": started_at,
            "durationSeconds": record.get("durationSeconds", 0),
            "timestamps": [{"value": started_at, "label": "Started"}],
            "source": {"filePath": path},
        })
    return artifacts


def run(context):
    path = context["file"]["path"]
    folder = os.path.basename(os.path.dirname(path)).lower()

    try:
        records = _load_json(path)
    except (OSError, ValueError) as error:
        return {"kind": "record", "label": f"Unreadable export: {error}", "fields": {"path": path}}

    if not isinstance(records, list):
        return []

    if folder == "messages":
        return _parse_messages(path, records)
    if folder == "contacts":
        return _parse_contacts(path, records)
    if folder == "calls":
        return _parse_calls(path, records)
    return []
