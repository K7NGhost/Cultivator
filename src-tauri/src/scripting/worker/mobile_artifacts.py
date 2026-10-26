"""Cultivator JSON-lines adapter for the bundled, unmodified LEAPP parsers.

Parser stdout is isolated from the protocol. Requests for evidence bytes are
served by the parent Inodex reader; upstream seekers create parser working copies.
"""
import base64
import contextlib
import fnmatch
import glob
import importlib
import json
import math
import os
from pathlib import Path
import re
import sqlite3
import sys
import tempfile
import time
import traceback
from datetime import datetime, timezone

PROTOCOL = sys.stdout
INPUT = sys.stdin


def emit(event):
    PROTOCOL.write(json.dumps(event, ensure_ascii=True, allow_nan=False) + "\n")
    PROTOCOL.flush()


class LogStream:
    def __init__(self):
        self.pending = ""

    def write(self, text):
        self.pending += text
        while "\n" in self.pending:
            line, self.pending = self.pending.split("\n", 1)
            if line.strip():
                emit({"type": "log", "level": "info", "message": line.rstrip("\r")})
        return len(text)

    def flush(self):
        if self.pending.strip():
            emit({"type": "log", "level": "info", "message": self.pending})
        self.pending = ""


def path_key(path):
    return os.path.normcase(os.path.normpath(str(path)))


def materialize(paths):
    paths = list(dict.fromkeys(str(p) for p in paths))
    for start in range(0, len(paths), 128):
        emit({"type": "materialize", "paths": paths[start:start + 128]})
        line = INPUT.readline()
        if not line:
            raise RuntimeError("Evidence reader disconnected")
        response = json.loads(line)
        if not response.get("ok"):
            raise RuntimeError(response.get("error", "Evidence materialization failed"))


def evidence_seeker(base, index):
    entries = {path_key(item["path"]): item for item in index}

    class EvidenceSeeker(base):
        _cultivator_index = index

        def build_files_list(self, directory):
            self._all_files = [item["path"] for item in index]

        def search(self, filepattern, return_on_first_hit=False, force=False):
            if filepattern in self.searched and not force:
                return super().search(filepattern, return_on_first_hit, force)
            pattern = fnmatch.translate(os.path.normcase(filepattern))
            matcher = re.compile(pattern).match
            matched = [item for item in index
                       if matcher(os.path.normcase("root/") + os.path.normcase(item["path"]))]
            if return_on_first_hit:
                matched = matched[:1]
            files = [item["path"] for item in matched if item.get("kind") == "file"]
            directories = [item["path"] for item in matched if item.get("kind") == "directory"]
            for directory in directories:
                prefix = path_key(directory) + os.sep
                files.extend(item["path"] for item in index
                             if item.get("kind") == "file" and path_key(item["path"]).startswith(prefix))
            companions = []
            for path in files:
                for suffix in ("-wal", "-shm", "-journal"):
                    item = entries.get(path_key(path + suffix))
                    if item and item.get("kind") == "file":
                        companions.append(item["path"])
            materialize(files + companions)
            result = super().search(filepattern, return_on_first_hit, force)
            for directory in directories:
                os.makedirs(os.path.join(self.data_folder, os.path.relpath(directory, self.directory)), exist_ok=True)
                super().search(glob.escape("root/" + directory) + os.sep + "*")
            # SQLite companions must also reach the parser's own staged directory.
            for companion in companions:
                super().search(glob.escape("root/" + companion))
            return result

    EvidenceSeeker.__name__ = base.__name__
    return EvidenceSeeker


def itunes_seeker(base, index):
    entries = {path_key(item["path"]): item for item in index}

    class ItunesSeeker(base):
        def search(self, filepattern, return_on_first_hit=False, force=False):
            if filepattern in self.searched and not force:
                return super().search(filepattern, return_on_first_hit, force)
            keys = fnmatch.filter(self._all_files, filepattern)
            if return_on_first_hit:
                keys = keys[:1]
            companions = [key + suffix for key in keys for suffix in ("-wal", "-shm", "-journal")
                          if key + suffix in self._all_files]
            locations = []
            for relative in keys + companions:
                filename = self._all_files[relative]
                location = Path(self.directory) / filename[:2] / filename if self.backup_type == "db" else Path(self.directory) / filename
                locations.append(str(location))
                source = entries.get(path_key(location))
                if source and not any(item["relative"] == relative for item in index):
                    index.append({**source, "relative": relative})
            materialize(locations)
            result = super().search(filepattern, return_on_first_hit, force)
            for relative in companions:
                super().search(glob.escape(relative))
            return result

    ItunesSeeker.__name__ = base.__name__
    return ItunesSeeker


def install_inventory_adapter(loader):
    """Supply the alternate inventory parsers with unextracted Inodex metadata."""
    patched = set()
    for plugin in loader.plugins:
        function = plugin.method
        while hasattr(function, "__wrapped__"):
            function = function.__wrapped__
        namespace = getattr(function, "__globals__", {})
        if id(namespace) in patched or "_iter_extraction_files" not in namespace:
            continue
        original = namespace["_iter_extraction_files"]

        def records(seeker, original=original):
            index = getattr(seeker, "_cultivator_index", None)
            if index is None:
                yield from original(seeker)
                return
            for item in index:
                if item.get("kind") != "file":
                    continue
                metadata = item.get("provenance", {}).get("metadata")
                if metadata is None:
                    try:
                        status = os.stat(item["path"])
                        size, modified = status.st_size, datetime.fromtimestamp(status.st_mtime, timezone.utc).strftime("%Y-%m-%d %H:%M:%S")
                    except OSError:
                        size, modified = "", ""
                else:
                    size = metadata["size"]
                    timestamp = metadata.get("times", {}).get("modified")
                    try:
                        modified = datetime.fromtimestamp(timestamp["seconds"], timezone.utc).strftime("%Y-%m-%d %H:%M:%S") if timestamp else ""
                    except (ValueError, OSError, OverflowError):
                        modified = ""
                # Match the directory seeker's leading separator convention.
                yield "/" + item["relative"], size, modified

        namespace["_iter_extraction_files"] = records
        patched.add(id(namespace))


@contextlib.contextmanager
def native_tool_lock(directory):
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "download.lock").open("a+b") as lock:
        lock.seek(0)
        lock.write(b"0")
        lock.flush()
        lock.seek(0)
        if os.name == "nt":
            import msvcrt
            while True:
                try:
                    msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError:
                    time.sleep(0.1)
            try:
                yield
            finally:
                lock.seek(0)
                msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl
            fcntl.flock(lock, fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(lock, fcntl.LOCK_UN)


def install_unified_log_reader(bundle):
    """Provision upstream's digest-pinned native reader only when requested."""
    from scripts import unifiedlogs
    original = unifiedlogs.find_iterator

    def verified_reader():
        helper = bundle / "admin/scripts/fetch_unifiedlog_iterator.py"
        spec = importlib.util.spec_from_file_location("cultivator_unifiedlogs_fetch", helper)
        fetcher = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fetcher)
        key = fetcher.current_platform()
        name = "unifiedlog_iterator.exe" if key.startswith("windows") else "unifiedlog_iterator"
        directory = bundle / "native-tools" / key
        binary = directory / name
        manifest = directory / "verified.json"
        if manifest.is_file() and binary.is_file():
            from hashlib import sha256
            details = json.loads(manifest.read_text(encoding="utf-8"))
            if details.get("version") == fetcher.PINNED_VERSION and details.get("sha256") == sha256(binary.read_bytes()).hexdigest():
                return str(binary)
        emit({"type": "log", "level": "info", "message": "Preparing the verified native Unified Logs reader (first use downloads it from its upstream release)."})
        directory.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="download-", dir=directory) as temporary:
            fetcher.BIN_DIR = Path(temporary)
            fetcher.fetch(key)  # Checks the pinned archive SHA-256 before extraction.
            from hashlib import sha256
            details = {"version": fetcher.PINNED_VERSION,
                       "archiveSha256": fetcher.ASSETS[key][1],
                       "sha256": sha256((Path(temporary) / name).read_bytes()).hexdigest()}
            for filename in ("LICENSE-unifiedlog_iterator", name):
                os.replace(Path(temporary) / filename, directory / filename)
            ready = Path(temporary) / "verified.json"
            ready.write_text(json.dumps(details), encoding="utf-8")
            os.replace(ready, manifest)
        return str(binary)

    def find_iterator():
        existing = original()
        if existing:
            return existing
        if os.environ.get(unifiedlogs.BINARY_ENV_VAR):
            raise RuntimeError("The configured Unified Logs reader is not executable")
        with native_tool_lock(bundle / "native-tools"):
            return verified_reader()

    unifiedlogs.find_iterator = find_iterator


def load_catalog(bundle):
    from scripts.plugin_loader import PluginLoader
    catalog = json.loads((bundle / "catalog.json").read_text(encoding="utf-8"))
    paths = [bundle / "scripts/artifacts", bundle / "scripts/alternate_artifacts"]
    loader = PluginLoader(plugin_paths=paths)
    expected = {artifact["key"] for module in catalog["modules"] for artifact in module["artifacts"]}
    actual = {plugin.name for plugin in loader.plugins}
    if actual != expected:
        raise RuntimeError(f"Parser catalog mismatch: missing={sorted(expected - actual)}, "
                           f"unexpected={sorted(actual - expected)}")
    return loader, catalog


def json_value(value):
    if isinstance(value, bytes):
        return {"encoding": "base64", "data": base64.b64encode(value).decode("ascii")}
    if isinstance(value, float) and not math.isfinite(value):
        return {"type": "float", "value": str(value)}
    return value


def import_tables(output, root, index, catalog, source_number):
    metadata = json.loads((output / "_lava_data.lava").read_text(encoding="utf-8"))
    lookup = {item["relative"].replace("\\", "/"): item for item in index}
    database = sqlite3.connect((output / "_lava_artifacts.db").resolve().as_uri() + "?mode=ro", uri=True)
    rows_written = 0
    try:
        for category, artifacts in metadata["artifacts"].items():
            for artifact in artifacts:
                table = artifact["tablename"]
                quoted = '"' + table.replace('"', '""') + '"'
                cursor = database.execute(f"SELECT * FROM {quoted} ORDER BY rowid")
                columns = [{"key": field[0], "label": field[0]} for field in cursor.description]
                # Keep the original display labels when LAVA's column map supplies them.
                mapping = artifact.get("column_map", {})
                if isinstance(mapping, dict):
                    for column in columns:
                        column["label"] = mapping.get(column["key"], column["key"])
                source_path = artifact.get("source_path", "")
                reported_paths = source_path if isinstance(source_path, list) else str(source_path).splitlines()
                sources = []
                for reported in reported_paths:
                    normalized = str(reported).replace("\\", "/")
                    try:
                        normalized = Path(reported).relative_to(output / "data").as_posix()
                    except ValueError:
                        pass
                    source = lookup.get(normalized)
                    if source and source not in sources:
                        sources.append(source)
                source = sources[0] if sources else next((entry for entry in index
                    if path_key(entry["path"]) == path_key(root) and entry.get("kind") == "file"), None)
                if source and source not in sources:
                    sources.append(source)
                source_reference = {"filePath": source["path"] if source else str(root),
                                    "parser": f"{catalog['project']}:{artifact.get('artifact_key', table)}"}
                offset = 0
                while True:
                    rows = cursor.fetchmany(256)
                    if not rows:
                        break
                    payload = {
                        "kind": "custom_table", "category": "other", "label": artifact["name"],
                        "source": source_reference,
                        "sources": [{"filePath": entry["path"], "provenance": entry.get("provenance")} for entry in sources],
                        "group": {"id": f"{catalog['platform']}:{source_number}:{table}", "label": category},
                        "table": {"name": f"{category} / {artifact['name']}", "columns": columns,
                                  "rows": [{column["key"]: json_value(value) for column, value in zip(columns, row)}
                                           for row in rows]},
                        "parser": {"project": catalog["project"], "revision": catalog["revision"],
                                   "artifact": artifact, "category": category, "rowOffset": offset,
                                   "outputDirectory": str(output)},
                        "provenance": source.get("provenance") if source else {"root": str(root)},
                    }
                    emit({"type": "artifact", "filePath": source_reference["filePath"],
                          "label": artifact["name"], "payload": payload})
                    offset += len(rows)
                    rows_written += len(rows)
    finally:
        database.close()
    failures = [item for item in metadata["modules"] if item["module_status"] == "Error"]
    return rows_written, failures, metadata["modules"]


def run(request, bundle):
    loader, catalog = load_catalog(bundle)
    emit({"type": "catalog", "artifacts": len(loader), "modules": catalog["counts"]["modules"],
          "platform": catalog["platform"], "revision": catalog["revision"]})
    if request.get("action") == "validate":
        emit({"type": "done", "rows": 0, "failures": [], "catalogVerified": True})
        return
    core = importlib.import_module({"ios": "ileapp", "android": "aleapp", "vehicle": "vleapp"}[catalog["platform"]])
    install_inventory_adapter(loader)
    if catalog["platform"] == "ios":
        install_unified_log_reader(bundle)
    from scripts.context import Context
    from scripts import lavafuncs
    import pytz

    timezone = request.get("options", {}).get("timezone", "UTC")
    pytz.timezone(timezone)  # Reject invalid configuration before reading evidence.
    # Cultivator presents LAVA rows directly. Preserve lava-only parser semantics.
    for plugin in loader.plugins:
        types = plugin.artifact_info.get("output_types", "")
        plugin.artifact_info["output_types"] = "lava_only" if "lava_only" in types else ["lava"]
    core.report.generate_report = lambda *args, **kwargs: None
    original_seeker = core.FileSeekerDir
    original_itunes = getattr(core, "FileSeekerItunes", None)
    total_rows, failures, coverage = 0, [], []
    roots = request["roots"]
    output_root = Path(request["outputRoot"])
    output_root.mkdir(parents=True, exist_ok=True)
    if catalog["platform"] == "vehicle":
        # Keep IVA unwraps and embedded image working files owned by the case,
        # including intermediates left behind if a worker is cancelled.
        temporary_root = output_root / "temporary"
        temporary_root.mkdir(exist_ok=True)
        tempfile.tempdir = str(temporary_root)
    for source_number, item in enumerate(roots):
        root = Path(item["path"])
        index = [json.loads(line) for line in Path(item["indexPath"]).read_text(encoding="utf-8").splitlines()]
        core.FileSeekerDir = evidence_seeker(original_seeker, index)
        extract_type = item.get("inputType", "fs")
        if extract_type == "iva":
            if catalog["platform"] != "vehicle":
                raise RuntimeError("IVA input requires the vehicle artifact plugin")
            materialize([str(root)])
        backup_password = None
        if catalog["platform"] == "ios":
            # iTunes physical filenames are hashes, so hydrate only the backup
            # manifests initially. The seeker requests matching hashed files later.
            markers = [entry["path"] for entry in index
                       if entry["relative"] in ("Info.plist", "Manifest.plist", "Manifest.db", "Manifest.mbdb")]
            materialize(markers)
            backup_type = core.get_itunes_backup_type(str(root))
            if backup_type:
                supported, encrypted, message = core.check_itunes_backup_status(str(root), backup_type)
                if not supported:
                    raise RuntimeError(message)
                if encrypted:
                    password_file = request.get("options", {}).get("backupPasswordFile", "")
                    if not password_file:
                        raise RuntimeError("Encrypted iTunes backup requires a backup password file in plugin options")
                    backup_password = Path(password_file).read_text(encoding="utf-8").rstrip("\r\n")
                    if not backup_password:
                        raise RuntimeError("Backup password file is empty")
                core.FileSeekerItunes = itunes_seeker(original_itunes, index)
                extract_type = "itunes"
            keychain = request.get("options", {}).get("keychainPath", "")
            Context.set_keychain_path(keychain or None)
        options = core.OutputParameters(str(output_root), f"source-{source_number + 1}")
        Context.set_output_params(options)
        lavafuncs.initialize_lava(str(root), options.output_folder_base, extract_type)
        plugins = list(sorted(loader.plugins, key=lambda plugin: (plugin.category, plugin.name)))
        if catalog["platform"] == "ios":
            # The upstream dispatcher invokes its logarchive children; these are
            # all registered and checked above, rather than executed twice.
            plugins = [p for p in plugins if p.name != "last_build"
                       and p.module_name != "iTunesBackupInfo"
                       and (p.module_name != "logarchive" or p.name == "logarchive")]
        def progress(completed, total):
            emit({"type": "progress", "completed": source_number * total + completed,
                  "total": len(roots) * total,
                  "message": f"{catalog['project']}: {completed}/{total} artifacts"})
        core.GuiWindow.SetProgressBar = staticmethod(progress)
        arguments = [plugins, extract_type, str(root), options, True, loader, {}]
        if catalog["platform"] == "ios":
            arguments.append(timezone)
        extra = {"itunes_backup_password": backup_password} if catalog["platform"] == "ios" else {}
        success = core.crunch_artifacts(*arguments, None, **extra)
        if not success:
            raise RuntimeError(f"{catalog['project']} could not process {root}")
        lavafuncs.lava_finalize_output(options.output_folder_base)
        rows, errors, modules = import_tables(Path(options.output_folder_base), root, index, catalog,
                                             request.get("sourceIndex", 0) + source_number)
        total_rows += rows
        failures.extend(errors)
        coverage.append({"root": str(root), "modules": modules, "rows": rows})
    (output_root / "coverage.json").write_text(json.dumps(coverage, indent=2), encoding="utf-8")
    emit({"type": "done", "rows": total_rows, "failures": failures, "catalogVerified": True})


def main():
    global PROTOCOL
    # Native libraries and parser subprocesses can write directly to descriptor
    # 1, bypassing Python's redirect_stdout. Keep the protocol on its own handle.
    PROTOCOL = os.fdopen(os.dup(sys.stdout.fileno()), "w", encoding="utf-8", buffering=1)
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    bundle = Path(sys.argv[1]).resolve()
    sys.path.insert(0, str(bundle))
    request = json.loads(INPUT.readline())
    logs = LogStream()
    with contextlib.redirect_stdout(logs), contextlib.redirect_stderr(logs):
        try:
            run(request, bundle)
            logs.flush()
        except BaseException:
            logs.flush()
            emit({"type": "error", "message": traceback.format_exc()})
            raise SystemExit(1)


if __name__ == "__main__":
    main()
