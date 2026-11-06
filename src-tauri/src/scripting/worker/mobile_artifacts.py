"""Cultivator JSON-lines adapter for the bundled LEAPP parsers with managed-Python dependencies.

Parser stdout is isolated from the protocol. Requests for evidence bytes are
served by the parent Inodex reader; upstream seekers create parser working copies.
"""
import base64
from bisect import bisect_left
from collections import defaultdict
from copy import copy
import contextlib
import fnmatch
import glob
import importlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import sqlite3
import sys
import tempfile
import tarfile
import traceback
import time
import zipfile
from datetime import datetime, timezone

PROTOCOL = sys.stdout
INPUT = sys.stdin


def create_normalizer(request=None):
    request = request or {}
    config = request.get("normalization", {})
    base = Path(__file__).parent if "__file__" in globals() else None
    module_path = Path(config["modulePath"]) if config.get("modulePath") else base / "leapp_normalization.py"
    spec = importlib.util.spec_from_file_location("cultivator_leapp_normalization", module_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    rules = Path(config["rulesPath"]) if config.get("rulesPath") else base / "leapp_normalization_rules.json"
    schemas = Path(config["schemasPath"]) if config.get("schemasPath") else base / "artifact_model_schemas.json"
    enabled = request.get("options", {}).get("normalizeArtifacts", True) not in (False, "false", "False", "0", 0)
    engine = module.LeappNormalizer(json.loads(rules.read_text(encoding="utf-8")), json.loads(schemas.read_text(encoding="utf-8")), enabled)
    return engine, module.normalization_report


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


def load_evidence_index(path):
    # JSON lines otherwise allocate the same metadata keys and source label for
    # every file. Share those strings and avoid a second full-file text buffer.
    def shared_fields(pairs):
        return {sys.intern(key): sys.intern(value)
                if key in ("kind", "source", "basis") and isinstance(value, str) else value
                for key, value in pairs}

    with Path(path).open(encoding="utf-8") as stream:
        return [json.loads(line, object_pairs_hook=shared_fields) for line in stream if line.strip()]


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


class EvidencePathIndex:
    """Prune glob candidates without changing fnmatch ordering or semantics."""

    def __init__(self, index):
        self.paths = []
        self.names = defaultdict(list)
        self.extensions = defaultdict(list)
        self.ordered_paths = []
        self.stats = {"searches": 0, "candidates": 0, "fullScanCandidates": 0,
                      "regexCandidates": 0, "matchingSeconds": 0.0, "stagingSeconds": 0.0}
        for ordinal, item in enumerate(index):
            normalized = os.path.normcase("root/" + item["path"])
            record = (item, normalized)
            self.paths.append(record)
            self.names[os.path.basename(normalized)].append(record)
            self.extensions[os.path.splitext(normalized)[1]].append(record)
            self.ordered_paths.append((path_key(item["path"]), ordinal, item))
        self.ordered_paths.sort(key=lambda entry: entry[0])
        self.path_keys = [key for key, _, _ in self.ordered_paths]

    def match(self, pattern, first=False):
        started = time.perf_counter()
        pattern = os.path.normcase(pattern)
        name = os.path.basename(pattern)
        extension = os.path.splitext(name)[1]
        candidates = self.paths
        if not glob.has_magic(name):
            candidates = self.names.get(name, ())
        elif extension and not glob.has_magic(extension):
            candidates = self.extensions.get(extension, ())
        matcher = re.compile(fnmatch.translate(pattern)).match
        # fnmatch's '*' also spans directories. A literal substring check is
        # safe even when basename-prefix pruning would lose such matches.
        literal = max(re.split(r"[*?]", pattern), key=len) if "[" not in pattern else ""
        matched = []
        examined = 0
        regex_candidates = 0
        for item, normalized in candidates:
            examined += 1
            if literal and literal not in normalized:
                continue
            regex_candidates += 1
            if matcher(normalized):
                matched.append(item)
                if first:
                    break
        self.stats["searches"] += 1
        self.stats["candidates"] += examined
        self.stats["regexCandidates"] += regex_candidates
        self.stats["fullScanCandidates"] += len(self.paths)
        self.stats["matchingSeconds"] += time.perf_counter() - started
        return matched

    def descendants(self, directory, files_only=False):
        prefix = path_key(directory) + os.sep
        position = bisect_left(self.path_keys, prefix)
        children = []
        while position < len(self.ordered_paths) and self.ordered_paths[position][0].startswith(prefix):
            _, ordinal, item = self.ordered_paths[position]
            if not files_only or item.get("kind") == "file":
                children.append((ordinal, item["path"]))
            position += 1
        yield from (path for _, path in sorted(children))


def archive_seeker(base, index, container, kind):
    """Index archive headers; let upstream stage only matching members and links."""
    source = next(item for item in index if path_key(item["path"]) == path_key(container))
    container_path = str(container)

    class ArchiveSeeker(base):
        def _read_from_a_decompressed_copy(self, *args):
            # Upstream normally writes the whole uncompressed TAR as a spool.
            # Read the compressed source in place instead; never create that copy.
            emit({"type": "log", "level": "info", "message":
                  "Reading compressed TAR in place; only parser matches will be extracted."})

        def _index_member_names(self):
            result = super()._index_member_names()
            if kind == "zip":
                return result
            members, versions = result
            self._original_names = {}
            def bounded(member):
                name = member.name.replace("\\", "/")
                if ".." in name.split("/"):
                    return None
                # Forensic TARs can store absolute device paths. Strip their
                # leading slash so upstream cannot write outside data_folder.
                name = name.lstrip("/")
                if not name or ":" in name.split("/", 1)[0]:
                    return None
                self._original_names[name] = member.name
                safe = copy(member)
                safe.name = name
                return safe
            safe_members = [safe for member in members if (safe := bounded(member)) is not None]
            safe_versions = {}
            for name, duplicates in versions.items():
                entries = [(ordinal, safe) for ordinal, member in duplicates
                           if (safe := bounded(member)) is not None]
                if entries:
                    safe_versions[entries[0][1].name] = entries
            return safe_members, safe_versions

        def __init__(self, path, data_folder):
            super().__init__(path, data_folder)
            self._candidate_attribute = "_search_names" if kind == "zip" else "_search_members"
            self._archive_candidates = list(getattr(self, self._candidate_attribute))
            self._members_by_name = {member if kind == "zip" else member.name: member
                                     for member in self._archive_candidates}
            records = {}
            self._synthetic_directories = set()
            for name, member in self._members_by_name.items():
                directory = name.endswith("/") if kind == "zip" else member.isdir()
                record_name = name.rstrip("/") if directory else name
                if not record_name:
                    continue
                records[record_name] = self._record(record_name, "directory" if directory else "file", member)
                if record_name != name:
                    self._synthetic_directories.add(record_name)
            # ZIPs need not contain explicit directory entries. Parsers searching
            # for a folder must still receive that folder and its children.
            for name in list(records):
                parent = name.rstrip("/").rsplit("/", 1)[0] if "/" in name.rstrip("/") else ""
                while parent:
                    if parent not in records and parent + "/" not in records:
                        records[parent] = self._record(parent, "directory")
                        self._synthetic_directories.add(parent)
                    parent = parent.rsplit("/", 1)[0] if "/" in parent else ""
            self._archive_lookup = EvidencePathIndex(list(records.values()))
            type(self)._cultivator_lookup = self._archive_lookup
            # Retain the container record for artifacts without a source member.
            index.extend(records.values())

        def _record(self, name, entry_kind, member=None):
            info = (self._chosen.get(member) or self.zip_file.getinfo(member)) if kind == "zip" and member is not None else member
            size = (info.file_size if kind == "zip" else info.size) if info is not None else 0
            metadata = {"size": size, "kind": entry_kind}
            if kind == "tar" and info is not None:
                metadata["times"] = {"modified": {"seconds": info.mtime, "basis": "utc"}}
            original_name = member if kind == "zip" and member is not None else getattr(self, "_original_names", {}).get(name, name)
            provenance = {**source.get("provenance", {}), "archiveMember": original_name,
                          "archivePath": container_path, "metadata": metadata,
                          "containerProvenance": source.get("provenance", {})}
            return {"path": name, "relative": name.replace("\\", "/"), "kind": entry_kind,
                    "containerPath": container_path, "provenance": provenance}

        def _stage(self, pattern, names, first=False, force=False):
            all_candidates = getattr(self, self._candidate_attribute)
            setattr(self, self._candidate_attribute,
                    [self._members_by_name[name] for name in names if name in self._members_by_name])
            started = time.perf_counter()
            try:
                return super().search(pattern, first, force)
            finally:
                setattr(self, self._candidate_attribute, all_candidates)
                self._archive_lookup.stats["stagingSeconds"] += time.perf_counter() - started

        def search(self, pattern, return_on_first_hit=False, force=False):
            if pattern in self.searched and not force:
                return super().search(pattern, return_on_first_hit, force)
            matched = self._archive_lookup.match(pattern, return_on_first_hit)
            names = [item["path"] for item in matched]
            result = self._stage(pattern, names, return_on_first_hit, force)
            directories = [item["path"] for item in matched if item["kind"] == "directory"]
            for directory in directories:
                if directory in self._synthetic_directories:
                    from scripts.search_files import sanitize_file_path
                    parts = [part for part in sanitize_file_path(directory).replace("\\", "/").split("/")
                             if part not in ("", ".", "..")]
                    folder = os.path.join(self.data_folder, *parts)
                    os.makedirs(folder, exist_ok=True)
                    if return_on_first_hit:
                        result = folder
                    else:
                        result.append(folder)
                children = list(self._archive_lookup.descendants(directory))
                self._stage(glob.escape("root/" + directory.rstrip("/")) + "/*", children, force=force)
                names.extend(children)
            # SQLite WAL/SHM/journal files belong beside the matched database.
            for name in dict.fromkeys(names):
                for suffix in ("-wal", "-shm", "-journal"):
                    companion = name + suffix
                    if companion in self._members_by_name:
                        self._stage(glob.escape("root/" + companion), [companion], force=force)
            self.searched[pattern] = [result] if isinstance(result, str) else result
            return result

    ArchiveSeeker.__name__ = base.__name__
    return ArchiveSeeker


def evidence_seeker(base, index):
    entries = {path_key(item["path"]): item for item in index}
    lookup = EvidencePathIndex(index)

    class EvidenceSeeker(base):
        _cultivator_index = index
        _cultivator_lookup = lookup

        def build_files_list(self, directory):
            self._all_files = [item["path"] for item in index]

        def search(self, filepattern, return_on_first_hit=False, force=False):
            if filepattern in self.searched and not force:
                return super().search(filepattern, return_on_first_hit, force)
            matched = lookup.match(filepattern, return_on_first_hit)
            files = [item["path"] for item in matched if item.get("kind") == "file"]
            directories = [item["path"] for item in matched if item.get("kind") == "directory"]
            for directory in directories:
                files.extend(lookup.descendants(directory, files_only=True))
            companions = []
            for path in files:
                for suffix in ("-wal", "-shm", "-journal"):
                    item = entries.get(path_key(path + suffix))
                    if item and item.get("kind") == "file":
                        companions.append(item["path"])
            materialize(path for path in dict.fromkeys(files + companions)
                        if force or path not in self.copied)
            # Keep upstream copying, path guards, timestamps and search cache.
            # Only its input candidate list changes; always restore it on errors.
            def stage(pattern, paths, first=False, forced=False):
                started = time.perf_counter()
                all_files = self._all_files
                self._all_files = paths
                try:
                    return super(EvidenceSeeker, self).search(pattern, first, forced)
                finally:
                    self._all_files = all_files
                    lookup.stats["stagingSeconds"] += time.perf_counter() - started

            result = stage(filepattern, [item["path"] for item in matched], return_on_first_hit, force)
            for directory in directories:
                os.makedirs(os.path.join(self.data_folder, os.path.relpath(directory, self.directory)), exist_ok=True)
                stage(glob.escape("root/" + directory) + os.sep + "*", list(lookup.descendants(directory)), forced=force)
            # SQLite companions must also reach the parser's own staged directory.
            for companion in companions:
                stage(glob.escape("root/" + companion), [companion], forced=force)
            return result

    EvidenceSeeker.__name__ = base.__name__
    return EvidenceSeeker


def itunes_seeker(base, index):
    entries = {path_key(item["path"]): item for item in index}
    indexed_relatives = {item["relative"] for item in index}

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
                if source and relative not in indexed_relatives:
                    index.append({**source, "relative": relative})
                    indexed_relatives.add(relative)
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


def import_tables(output, root, index, catalog, source_number, normalizer=None):
    normalizer, report_for = normalizer or create_normalizer()
    plans = []
    metadata = json.loads((output / "_lava_data.lava").read_text(encoding="utf-8"))
    lookup = {item["relative"].replace("\\", "/"): item for item in index}
    root_key = path_key(root)
    root_source = next((entry for entry in index
                        if entry.get("kind") == "file" and path_key(entry["path"]) == root_key), None)
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
                plan = normalizer.plan(catalog, artifact, columns)
                plans.append(plan)
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
                source = sources[0] if sources else root_source
                if source and source not in sources:
                    sources.append(source)
                source_reference = {"filePath": source.get("containerPath", source["path"]) if source else str(root),
                                    "parser": f"{catalog['project']}:{artifact.get('artifact_key', table)}"}
                offset = 0
                while True:
                    rows = cursor.fetchmany(256)
                    if not rows:
                        break
                    payload = {
                        "kind": "custom_table", "category": "other", "label": artifact["name"],
                        "source": source_reference,
                        "sources": [{"filePath": entry.get("containerPath", entry["path"]), "provenance": entry.get("provenance")} for entry in sources],
                        "group": {"id": f"{catalog['platform']}:{source_number}:{table}", "label": category},
                        "table": {"name": f"{category} / {artifact['name']}", "columns": columns,
                                  "rows": [{column["key"]: json_value(value) for column, value in zip(columns, row)}
                                           for row in rows]},
                        "parser": {"project": catalog["project"], "revision": catalog["revision"],
                                   "artifact": artifact, "category": category, "rowOffset": offset,
                                   "outputDirectory": str(output)},
                        "provenance": source.get("provenance") if source else {"root": str(root)},
                    }
                    fallback = []
                    fallback_offset = None
                    def flush_fallback():
                        if fallback:
                            emit({"type": "artifact", "filePath": source_reference["filePath"], "label": artifact["name"],
                                  "payload": {**payload, "parser": {**payload["parser"], "rowOffset": fallback_offset},
                                              "table": {**payload["table"], "rows": list(fallback)}}})
                            fallback.clear()
                    for row_number, row in enumerate(payload["table"]["rows"], offset):
                        normalized = normalizer.normalize(plan, row, payload, row_number, artifact)
                        if normalized is not None:
                            flush_fallback()
                            emit({"type": "artifact", "filePath": source_reference["filePath"], "label": artifact["name"], "payload": normalized})
                        else:
                            if not fallback:
                                fallback_offset = row_number
                            fallback.append(row)
                    flush_fallback()
                    offset += len(rows)
                    rows_written += len(rows)
    finally:
        database.close()
    failures = [item for item in metadata["modules"] if item["module_status"] == "Error"]
    report = report_for(plans)
    report.update(project=catalog["project"], platform=catalog["platform"], sourceRevision=catalog["revision"])
    report_path = output / "normalization.json"
    report_path.write_text(json.dumps(report, indent=2, ensure_ascii=True), encoding="utf-8")
    emit({"type": "log", "level": "info", "message": f"Standardized {report['totals']['normalizedRows']} of {rows_written} LEAPP rows; {report['totals']['fallbackRows']} retained as custom tables. Mapping coverage: {report_path}"})
    return rows_written, failures, metadata["modules"]


def run(request, bundle):
    loader, catalog = load_catalog(bundle)
    emit({"type": "catalog", "artifacts": len(loader), "modules": catalog["counts"]["modules"],
          "platform": catalog["platform"], "revision": catalog["revision"]})
    if request.get("action") == "validate":
        emit({"type": "done", "rows": 0, "failures": [], "catalogVerified": True})
        return
    normalizer = create_normalizer(request)
    core = importlib.import_module({"ios": "ileapp", "android": "aleapp", "vehicle": "vleapp"}[catalog["platform"]])
    install_inventory_adapter(loader)
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
    original_zip, original_tar = core.FileSeekerZip, core.FileSeekerTar
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
        index = load_evidence_index(item["indexPath"])
        core.FileSeekerDir = evidence_seeker(original_seeker, index)
        extract_type = item.get("inputType") or ("fs" if root.is_dir() else
                       "iva" if catalog["platform"] == "vehicle" and root.suffix.lower() == ".iva" else "archive")
        if extract_type in ("archive", "zip", "tar", "gz"):
            materialize([str(root)])
            if zipfile.is_zipfile(root):
                extract_type = "zip"
                core.FileSeekerZip = archive_seeker(original_zip, index, root, "zip")
            elif tarfile.is_tarfile(root):
                extract_type = "tar"
                core.FileSeekerTar = archive_seeker(original_tar, index, root, "tar")
            else:
                raise RuntimeError(f"Unsupported or damaged archive: {root}. Expected ZIP or TAR (including compressed TAR).")
        if extract_type == "iva":
            if catalog["platform"] != "vehicle":
                raise RuntimeError("IVA input requires the vehicle artifact plugin")
            materialize([str(root)])
        backup_password = None
        if catalog["platform"] == "ios" and extract_type == "fs":
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
        if catalog["platform"] == "ios":
            keychain = request.get("options", {}).get("keychainPath", "")
            Context.set_keychain_path(keychain or None)
        options = core.OutputParameters(str(output_root), f"source-{source_number + 1}")
        Context.set_output_params(options)
        lavafuncs.initialize_lava(str(root), options.output_folder_base, extract_type)
        plugins = list(sorted(loader.plugins, key=lambda plugin: (plugin.category, plugin.name)))
        if catalog["platform"] == "ios":
            # The upstream dispatcher adds device-information parsers itself.
            plugins = [p for p in plugins if p.name != "last_build"
                       and p.module_name != "iTunesBackupInfo"]
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
        if extract_type in ("fs", "zip", "tar"):
            seeker_class = {"fs": core.FileSeekerDir, "zip": core.FileSeekerZip, "tar": core.FileSeekerTar}[extract_type]
            stats = seeker_class._cultivator_lookup.stats
            (Path(options.output_folder_base) / "search-performance.json").write_text(
                json.dumps(stats, indent=2), encoding="utf-8")
        rows, errors, modules = import_tables(Path(options.output_folder_base), root, index, catalog,
                                             request.get("sourceIndex", 0) + source_number, normalizer)
        total_rows += rows
        failures.extend(errors)
        report = json.loads((Path(options.output_folder_base) / "normalization.json").read_text(encoding="utf-8"))
        coverage.append({"root": str(root), "modules": modules, "rows": rows,
                         "normalization": report["totals"], "normalizationReport": str(Path(options.output_folder_base) / "normalization.json")})
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
