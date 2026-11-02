import contextlib
import csv
import datetime
import hashlib
import importlib.util
import io
import json
import os
import re
import sys
import types
import traceback
import threading

# The protocol and diagnostics have the same encoding on every platform.
sys.stdout.reconfigure(encoding="utf-8")
sys.stderr.reconfigure(encoding="utf-8")

ARTIFACT_CATEGORIES = {
    "refined_result": "search",
    "social_profile": "social",
    "social_post": "social",
    "document": "files",
    "encrypted_item": "credentials",
    "connected_device": "networks",
    "account": "accounts",
    "application": "applications",
    "browser_history": "browser",
    "call": "calls",
    "contact": "contacts",
    "credential": "credentials",
    "file": "files",
    "location": "locations",
    "media": "media",
    "message": "messages",
    "sms": "messages",
    "mms": "messages",
    "instant_message": "messages",
    "email": "communications",
    "chat": "communications",
    "voice_mail": "communications",
    "note": "notes",
    "calendar_entry": "calendar",
    "user_account": "accounts",
    "password": "credentials",
    "journey": "journeys",
    "installed_application": "applications",
    "cookie": "browser",
    "application_usage": "applications",
    "visited_page": "browser",
    "dictionary_word": "search",
    "web_bookmark": "browser",
    "shared_file": "files",
    "bluetooth_device": "networks",
    "map": "maps",
    "searched_item": "search",
    "wireless_network": "networks",
    "notification": "system",
    "carved_string": "search",
    "powering_event": "system",
    "system": "system",
    "timeline_event": "timeline",
    "custom_table": "other",
    "record": "other",
}

logs = []
artifacts = []
search_roots = []
evidence_sources = {}
evidence_requests = threading.Lock()


def evidence_path_key(path):
    return os.path.normcase(os.path.normpath(path)).removeprefix("\\\\?\\")


def load_evidence_sources(input_data):
    manifest = os.path.join(input_data["case_folder_path"], "artifacts", "evidence",
                            input_data["datasource"]["id"], "manifest.ndjson")
    if not os.path.isfile(manifest):
        return
    with open(manifest, encoding="utf-8") as source:
        for line in source:
            record = json.loads(line)
            path = record.get("localPath")
            if path:
                evidence_sources[evidence_path_key(path)] = record


def artifact_category_for_kind(kind):
    return ARTIFACT_CATEGORIES.get(kind)


def artifact_label_from_payload(payload):
    for key in ("name", "title", "url", "path", "key", "eventType"):
        value = payload.get(key)
        if isinstance(value, str):
            return value
    return ""


def json_record(payload, file_path):
    if not isinstance(payload, dict):
        payload = {"kind": "record", "label": str(payload), "value": payload}

    kind = str(payload.get("kind") or "record")
    payload.setdefault("kind", kind)
    category = artifact_category_for_kind(kind)

    if category:
        payload.setdefault("category", category)

    return {
        "filePath": file_path,
        "resultKind": kind,
        "label": str(payload.get("label") or artifact_label_from_payload(payload)),
        "payload": payload,
    }


def normalize_result(result, file_path):
    if result is None:
        return []

    if isinstance(result, list):
        records = []
        for item in result:
            if item is not None:
                records.append(json_record(item, file_path))
        return records

    return [json_record(result, file_path)]


def ensure_local(path):
    """Return a readable working path, extracting indexed evidence only on demand."""
    path = os.path.abspath(path)
    if os.path.isfile(path):
        return path
    with evidence_requests:
        print("__CULTIVATOR_EVIDENCE__" + json.dumps({"path": os.fspath(path)}), file=sys.stderr, flush=True)
        response = sys.stdin.buffer.readline()
        if not response:
            raise OSError("Evidence reader connection closed")
        response = json.loads(response)
        if response.get("error"):
            raise OSError(response["error"])
        return response["path"]


def read_bytes(path, max_bytes=None):
    with open(ensure_local(path), "rb") as file:
        return file.read() if max_bytes is None else file.read(max_bytes)


def read_text(path, max_bytes=None, encoding="utf-8", errors="replace"):
    data = read_bytes(path, max_bytes)
    return data.decode(encoding, errors=errors)


def read_lines(path, encoding="utf-8", errors="replace"):
    return open(ensure_local(path), "r", encoding=encoding, errors=errors)


def sha256(path):
    digest = hashlib.sha256()
    with open(ensure_local(path), "rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def log(level, message):
    logs.append({"level": str(level), "message": str(message)})


def progress(completed, total, message=None, eta_seconds=None):
    completed = int(completed)
    total = int(total)
    if total <= 0 or completed < 0 or completed > total:
        raise ValueError("progress requires 0 <= completed <= total and total > 0")
    payload = {"completed": completed, "total": total}
    if message is not None:
        payload["message"] = str(message)
    if eta_seconds is not None:
        payload["etaSeconds"] = max(0, int(eta_seconds))
    print("__CULTIVATOR_PROGRESS__" + json.dumps(payload, separators=(",", ":")), file=sys.stderr, flush=True)


def create_artifact(kind, label, **fields):
    artifact = {"kind": kind}
    category = artifact_category_for_kind(kind)
    if category:
        artifact["category"] = category
    artifact["label"] = label
    artifact.update(fields)
    return artifact


def normalize_custom_category(category):
    value = re.sub(r"[^0-9A-Za-z]+", "_", str(category).strip()).strip("_").lower()
    return value or "other"


def table_header_key(label, existing):
    base = re.sub(r"[^0-9A-Za-z]+", "_", str(label).strip()).strip("_").lower() or "column"
    key = base
    suffix = 2
    while key in existing:
        key = f"{base}_{suffix}"
        suffix += 1
    existing.add(key)
    return key


def create_table_artifact(name, category, headers, label=None, icon=None, **fields):
    existing = set()
    columns = []
    for header in headers:
        if isinstance(header, dict):
            header_label = str(header.get("label") or header.get("key") or "")
            key = str(header.get("key") or table_header_key(header_label, existing))
            existing.add(key)
        else:
            header_label = str(header)
            key = table_header_key(header_label, existing)
        columns.append({"key": key, "label": header_label})

    artifact = create_artifact(
        "custom_table",
        label or str(name),
        category=normalize_custom_category(category),
        table={"name": str(name), "columns": columns, "rows": []},
        **fields,
    )
    if icon:
        artifact["icon"] = str(icon)
    return artifact


def add_table_row(table, values=None, **fields):
    row = {}
    if values:
        row.update(values)
    row.update(fields)
    table.setdefault("table", {}).setdefault("rows", []).append(row)


def create_group(label, id=None):
    return {
        "id": str(id or normalize_custom_category(label)),
        "label": str(label),
    }


def add_artifact(artifact, file_path=None, group=None):
    if isinstance(artifact, dict):
        if group is not None:
            artifact["group"] = group
        source = artifact.get("source")
        if not file_path and isinstance(source, dict):
            file_path = source.get("filePath")
        if not file_path:
            file_path = artifact.get("path")
    artifacts.append({"filePath": file_path, "payload": artifact})


def datetime_from_unix(ts):
    return datetime.datetime.fromtimestamp(float(ts)).isoformat()


def datetime_from_iso(text):
    return datetime.datetime.fromisoformat(str(text).strip().replace("Z", "+00:00")).isoformat()


def datetime_from_datetime(value):
    return value.isoformat()


def refined_result(label, **fields): return create_artifact("refined_result", label, **fields)
def social_profile(label, **fields): return create_artifact("social_profile", label, **fields)
def social_post(label, **fields): return create_artifact("social_post", label, **fields)
def document(label, **fields): return create_artifact("document", label, **fields)
def encrypted_item(label, **fields): return create_artifact("encrypted_item", label, **fields)
def connected_device(label, **fields): return create_artifact("connected_device", label, **fields)
def account(label, **fields): return create_artifact("account", label, **fields)
def application(label, **fields): return create_artifact("application", label, **fields)
def browser_history(label, **fields): return create_artifact("browser_history", label, **fields)
def call(label, **fields): return create_artifact("call", label, **fields)
def contact(label, **fields): return create_artifact("contact", label, **fields)
def credential(label, **fields): return create_artifact("credential", label, **fields)
def file_artifact(label, **fields): return create_artifact("file", label, **fields)
def location(label, **fields): return create_artifact("location", label, **fields)
def media(label, **fields): return create_artifact("media", label, **fields)
def message(label, **fields): return create_artifact("message", label, **fields)
def sms(label, **fields): return create_artifact("sms", label, **fields)
def mms(label, **fields): return create_artifact("mms", label, **fields)
def email(label, **fields): return create_artifact("email", label, **fields)
def chat(label, **fields): return create_artifact("chat", label, **fields)
def instant_message(label, **fields): return create_artifact("instant_message", label, **fields)
def user_account(label, **fields): return create_artifact("user_account", label, **fields)
def voice_mail(label, **fields): return create_artifact("voice_mail", label, **fields)
def calendar_entry(label, **fields): return create_artifact("calendar_entry", label, **fields)
def password(label, **fields): return create_artifact("password", label, **fields)
def journey(label, **fields): return create_artifact("journey", label, **fields)
def installed_application(label, **fields): return create_artifact("installed_application", label, **fields)
def cookie(label, **fields): return create_artifact("cookie", label, **fields)
def application_usage(label, **fields): return create_artifact("application_usage", label, **fields)
def visited_page(label, **fields): return create_artifact("visited_page", label, **fields)
def dictionary_word(label, **fields): return create_artifact("dictionary_word", label, **fields)
def web_bookmark(label, **fields): return create_artifact("web_bookmark", label, **fields)
def shared_file(label, **fields): return create_artifact("shared_file", label, **fields)
def bluetooth_device(label, **fields): return create_artifact("bluetooth_device", label, **fields)
def map_artifact(label, **fields): return create_artifact("map", label, **fields)
def searched_item(label, **fields): return create_artifact("searched_item", label, **fields)
def wireless_network(label, **fields): return create_artifact("wireless_network", label, **fields)
def notification(label, **fields): return create_artifact("notification", label, **fields)
def carved_string(label, **fields): return create_artifact("carved_string", label, **fields)
def powering_event(label, **fields): return create_artifact("powering_event", label, **fields)
def note(label, **fields): return create_artifact("note", label, **fields)
def system_artifact(label, **fields): return create_artifact("system", label, **fields)
def timeline_event(label, **fields): return create_artifact("timeline_event", label, **fields)


def search_files(root_path, query, regex=False, case_sensitive=False, binary_files=False, max_matches=None):
    flags = 0 if case_sensitive else re.IGNORECASE
    pattern = re.compile(query if regex else re.escape(query), flags)
    matches = []

    if max_matches is not None and max_matches <= 0:
        return matches
    root_key = evidence_path_key(root_path).rstrip(os.sep)
    indexed = [record["localPath"] for key, record in evidence_sources.items()
               if record.get("metadata", {}).get("kind") == "file"
               and (key == root_key or key.startswith(root_key + os.sep))]
    paths = indexed if indexed else ([root_path] if os.path.isfile(root_path) else
             (os.path.join(root, name) for root, _, files in os.walk(root_path) for name in files))
    for path in paths:
        try:
            text = read_text(path)
        except (OSError, UnicodeError):
            continue
        if not binary_files and "\x00" in text:
            continue
        for line_index, line in enumerate(text.splitlines(), 1):
            for match in pattern.finditer(line):
                matches.append({
                    "path": path,
                    "file": os.path.basename(path),
                    "line": line_index,
                    "column": match.start() + 1,
                    "matched_text": match.group(0),
                    "context": line,
                })
                if max_matches is not None and len(matches) >= max_matches:
                    return matches
    return matches


def search(query, regex=False, case_sensitive=False, binary_files=False, max_matches=None):
    matches = []
    for root in search_roots:
        remaining = None if max_matches is None else max_matches - len(matches)
        if remaining is not None and remaining <= 0:
            break
        matches.extend(search_files(root, query, regex, case_sensitive, binary_files, remaining))
    return matches


api = types.ModuleType("cultivator_api")
for name, value in list(globals().items()):
    if name in {
        "ensure_local", "read_bytes", "read_text", "read_lines", "sha256", "log", "progress", "create_artifact",
        "create_table_artifact", "add_table_row", "add_artifact", "create_group",
        "datetime_from_unix", "datetime_from_iso", "datetime_from_datetime", "account",
        "application", "browser_history", "call", "contact", "credential",
        "refined_result", "social_profile", "social_post", "document", "encrypted_item", "connected_device",
        "file_artifact", "location", "media", "message", "note",
        "sms", "mms", "email", "chat", "instant_message", "user_account",
        "voice_mail", "calendar_entry", "password", "journey",
        "installed_application", "cookie", "application_usage", "visited_page",
        "dictionary_word", "web_bookmark", "shared_file", "bluetooth_device",
        "map_artifact", "searched_item", "wireless_network", "notification",
        "carved_string", "powering_event", "system_artifact", "timeline_event",
        "search_files", "search",
    }:
        setattr(api, name, value)
sys.modules["cultivator_api"] = api


def load_plugin(plugin, plugin_directory):
    entry_path = os.path.join(plugin_directory, plugin["entry"])
    module_name = "cultivator_plugin_" + re.sub(r"\W+", "_", plugin["id"])
    sys.path.insert(0, plugin_directory)
    spec = importlib.util.spec_from_file_location(module_name, entry_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def build_context(input_data, task, matched_files):
    artifacts_path = os.path.join(input_data["case_folder_path"], "artifacts")
    target_file = task["target_file"]
    plugin = input_data["plugin"]
    datasource = input_data["datasource"]
    return {
        "case": {
            "id": datasource["case_id"],
            "database_path": input_data["case_database_path"],
            "folder_path": input_data["case_folder_path"],
            "artifacts_path": artifacts_path,
        },
        "datasource": {
            "id": datasource["id"],
            "name": datasource["name"],
            "paths": datasource["paths"],
        },
        "plugin": {
            "id": plugin["id"],
            "name": plugin["name"],
            "target": plugin["target"],
            "mode": plugin["mode"],
            "matched_files": matched_files,
        },
        "options": input_data.get("options", {}),
        "models": input_data.get("models", {}),
        "file": {
            "path": target_file["path"],
            "name": target_file["name"],
            "extension": target_file["extension"],
            "size": target_file["size"],
            "evidence": evidence_sources.get(evidence_path_key(target_file["path"])),
        },
        "task": {
            "plugin_id": task["plugin_id"],
            "file_path": task["file_path"],
            "datasource_id": task["datasource_id"],
            "case_id": task["case_id"],
        },
    }


def main():
    input_data = json.loads(sys.stdin.buffer.readline().decode("utf-8"))
    if input_data.get("protocol_version") != 1:
        raise ValueError("Unsupported Cultivator worker protocol")
    search_roots[:] = list(input_data["datasource"]["paths"])
    load_evidence_sources(input_data)
    with contextlib.redirect_stdout(sys.stderr):
        module = load_plugin(input_data["plugin"], input_data["plugin_directory"])
        function = getattr(module, input_data["plugin"]["function"])
    records = []
    matched_files = [item["file_path"] for item in input_data["tasks"]]

    for task in input_data["tasks"]:
        with contextlib.redirect_stdout(sys.stderr):
            result = function(build_context(input_data, task, matched_files))
        records.extend(normalize_result(result, task["file_path"]))

    for artifact in artifacts:
        file_path = artifact.get("filePath") or ""
        records.append(json_record(artifact.get("payload"), file_path))

    for record in records:
        evidence = evidence_sources.get(evidence_path_key(record["filePath"]))
        if evidence:
            record["payload"].setdefault("evidenceSource", evidence)

    json.dump({
        "scannedFiles": input_data["scanned_files"],
        "matchedFiles": len(input_data["tasks"]),
        "records": records,
        "logs": logs,
    }, sys.stdout, separators=(",", ":"))


if __name__ == "__main__":
    try:
        main()
    except Exception:
        traceback.print_exc(file=sys.stderr)
        sys.exit(1)
