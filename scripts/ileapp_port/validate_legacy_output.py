"""Exercise an old report-style parser through its rewritten Cultivator entrypoint."""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import tomllib
import types
from pathlib import Path
from typing import Any


class Capture:
    def __init__(self) -> None:
        self.artifacts: list[dict[str, Any]] = []

    def install(self) -> None:
        module = types.ModuleType("cultivator_api")
        module.log = lambda level, message: None
        module.read_bytes = lambda path, max_bytes=None: Path(path).read_bytes()[:max_bytes]
        module.read_text = lambda path, max_bytes=None, encoding="utf-8", errors="replace": Path(path).read_text(
            encoding=encoding,
            errors=errors,
        )[:max_bytes]
        module.read_lines = lambda path, encoding="utf-8", errors="replace": Path(path).read_text(
            encoding=encoding,
            errors=errors,
        ).splitlines(keepends=True)
        module.create_table_artifact = lambda name, category, headers, label=None, icon=None, **fields: {
            "kind": "custom_table",
            "name": name,
            "category": category,
            "headers": headers,
            "table": {"rows": []},
            **fields,
        }
        module.add_table_row = lambda table, values=None, **fields: table["table"]["rows"].append(
            {**(values or {}), **fields}
        )
        module.create_group = lambda label, id=None: {"label": label, "id": id or label}
        module.add_artifact = lambda artifact, file_path=None, group=None: self.artifacts.append(artifact)
        sys.modules["cultivator_api"] = module


def load_plugin(plugin_directory: Path):
    manifest = tomllib.loads((plugin_directory / "plugin.toml").read_text(encoding="utf-8"))
    spec = importlib.util.spec_from_file_location("legacy_audi_trip", plugin_directory / manifest["entry"])
    if spec is None or spec.loader is None:
        raise ImportError("Unable to load the legacy-output validation plugin")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module, manifest


def validate(plugins: Path) -> dict[str, int]:
    capture = Capture()
    capture.install()
    sys.path.insert(0, str(plugins / "_shared"))
    module, manifest = load_plugin(plugins / "auditripdata")

    with tempfile.TemporaryDirectory(prefix="cultivator-legacy-output-") as temporary:
        root = Path(temporary)
        evidence = root / "Library" / "Caches" / "de.audi.myaudimobileassistant" / "fsCachedData"
        evidence.mkdir(parents=True)
        source = evidence / "trip.json"
        source.write_text(
            json.dumps(
                {
                    "vehicle": {
                        "tripData": [
                            {
                                "timestamp": "2026-07-22T12:34:56Z",
                                "averageSpeed": 42,
                                "tripID": "trip-1",
                                "mileage": 12,
                                "overallMileage": 1200,
                                "startMileage": 1188,
                                "traveltime": 900,
                                "zeroEmissionDistance": 2,
                                "averageElectricEngineConsumption": 4.2,
                                "averageFuelConsumption": 7.1,
                                "tripType": "business",
                                "reportReason": "completed",
                            }
                        ]
                    }
                }
            ),
            encoding="utf-8",
        )
        case_folder = root / "case"
        context = {
            "case": {
                "id": "legacy",
                "database_path": str(case_folder / "case.sqlite"),
                "folder_path": str(case_folder),
                "artifacts_path": str(case_folder / "artifacts"),
            },
            "datasource": {"id": "legacy", "name": "Legacy", "paths": [str(root)]},
            "plugin": {
                "id": manifest["id"],
                "name": manifest["name"],
                "target": manifest["target"],
                "mode": manifest["mode"],
            },
            "options": {"timezone": "UTC"},
            "task": {
                "plugin_id": manifest["id"],
                "file_path": str(source),
                "datasource_id": "legacy",
                "case_id": "legacy",
            },
            "file": {
                "path": str(source),
                "name": source.name,
                "extension": "json",
                "size": source.stat().st_size,
            },
        }
        module.run(context)
        first_count = len(capture.artifacts)
        module.run(context)

    if first_count != 1 or len(capture.artifacts) != 1:
        raise RuntimeError("The old report parser did not emit exactly one table exactly once")
    table = capture.artifacts[0]
    if table["name"] != "Audi Trip Data" or len(table["table"]["rows"]) != 1:
        raise RuntimeError("The old report parser's table was not converted correctly")
    deduplication = table.get("deduplication", {})
    if deduplication.get("mode") != "group" or not deduplication.get("identityFields"):
        raise RuntimeError("The converted table is missing its explicit grouped-deduplication policy")
    return {"artifacts": 1, "rows": 1, "duplicateRuns": 0}


def main() -> int:
    plugins = Path(sys.argv[1]).resolve()
    print(json.dumps(validate(plugins), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
