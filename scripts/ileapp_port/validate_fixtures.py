"""Run rewritten plugins against the fixture archives bundled with iLEAPP."""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
import tempfile
import tomllib
import types
import zipfile
from pathlib import Path
from typing import Any


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="Path to the iLEAPP repository root")
    parser.add_argument("plugins", type=Path, help="Generated iLEAPP plugin folder")
    return parser.parse_args()


class ApiCapture:
    def __init__(self) -> None:
        self.artifacts: list[dict[str, Any]] = []
        self.logs: list[tuple[str, str]] = []

    def reset(self) -> None:
        self.artifacts.clear()
        self.logs.clear()

    def install(self) -> None:
        module = types.ModuleType("cultivator_api")
        module.log = lambda level, message: self.logs.append((str(level), str(message)))
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
            "label": label or name,
            "category": category,
            "headers": headers,
            "icon": icon,
            "table": {"rows": []},
            **fields,
        }
        module.add_table_row = lambda table, values=None, **fields: table["table"]["rows"].append(
            {**(values or {}), **fields}
        )
        module.create_group = lambda label, id=None: {"label": label, "id": id or label}

        def add_artifact(artifact, file_path=None, group=None):
            artifact["_filePath"] = file_path
            artifact["_group"] = group
            self.artifacts.append(artifact)

        module.add_artifact = add_artifact
        sys.modules["cultivator_api"] = module


def load_plugin(plugin_directory: Path, module_name: str):
    manifest = tomllib.loads((plugin_directory / "plugin.toml").read_text(encoding="utf-8"))
    spec = importlib.util.spec_from_file_location(
        "fixture_" + re.sub(r"\W+", "_", module_name),
        plugin_directory / manifest["entry"],
    )
    if spec is None or spec.loader is None:
        raise ImportError(f"Unable to load {plugin_directory}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module, manifest


def latest_expected_result(results: Path, module_name: str, artifact: str, case: str) -> dict[str, Any] | None:
    matches = sorted(results.glob(f"{module_name}.{artifact}.{case}.*.json"))
    if not matches:
        return None
    return json.loads(matches[-1].read_text(encoding="utf-8"))


def matching_table(module: Any, artifact_id: str, captured: list[dict[str, Any]]):
    spec = next((item for item in module._CULTIVATOR_ARTIFACTS if item["id"] == artifact_id), None)
    if spec:
        exact = [table for table in captured if table.get("name") == spec["name"]]
        if exact:
            return exact[0]
        return None
    return captured[0] if len(captured) == 1 else None


def run_fixture(
    source_root: Path,
    plugins: Path,
    module_name: str,
    artifact_id: str,
    case_name: str,
    zip_path: Path,
    capture: ApiCapture,
) -> tuple[int, list[tuple[str, str]], dict[str, Any] | None]:
    coverage = json.loads((plugins / "coverage.json").read_text(encoding="utf-8"))
    mapping = next(item for item in coverage["modules"] if item["sourceFile"] == f"{module_name}.py")
    plugin_directory = plugins / mapping["pluginDirectory"]
    capture.reset()
    module, manifest = load_plugin(plugin_directory, f"{module_name}_{artifact_id}_{case_name}")

    with tempfile.TemporaryDirectory(
        prefix="cultivator-ileapp-fixture-",
        ignore_cleanup_errors=True,
    ) as temporary:
        root = Path(temporary)
        evidence = root / "evidence"
        case_folder = root / "case"
        evidence.mkdir()
        case_folder.mkdir()
        with zipfile.ZipFile(zip_path) as archive:
            archive.extractall(evidence)
        files = sorted(str(path) for path in evidence.rglob("*") if path.is_file())
        context = {
            "case": {
                "id": "fixture",
                "database_path": str(case_folder / "case.sqlite"),
                "folder_path": str(case_folder),
                "artifacts_path": str(case_folder / "artifacts"),
            },
            "datasource": {"id": "fixture", "name": "Fixture", "paths": [str(evidence)]},
            "plugin": {
                "id": manifest["id"],
                "name": manifest["name"],
                "target": manifest["target"],
                "mode": manifest["mode"],
                "matched_files": files,
            },
            "options": {"timezone": "UTC"},
            "task": {
                "plugin_id": manifest["id"],
                "file_path": files[0],
                "datasource_id": "fixture",
                "case_id": "fixture",
            },
            "file": {
                "path": files[0],
                "name": Path(files[0]).name,
                "extension": Path(files[0]).suffix.lstrip("."),
                "size": Path(files[0]).stat().st_size,
            },
        }
        module.run(context)

    table = matching_table(module, artifact_id, capture.artifacts)
    row_count = len(table["table"]["rows"]) if table else 0
    return row_count, capture.logs, table


def validate_fixtures(source_root: Path, plugins: Path) -> dict[str, Any]:
    cases_root = source_root / "admin" / "test" / "cases"
    data_root = cases_root / "data"
    results_root = source_root / "admin" / "test" / "results"
    executed = 0
    compared = 0
    failures = []

    capture = ApiCapture()
    capture.install()
    sys.path.insert(0, str(plugins / "_shared"))
    for case_file in sorted(cases_root.glob("testdata.*.json")):
        module_name = case_file.stem.removeprefix("testdata.")
        test_cases = json.loads(case_file.read_text(encoding="utf-8"))
        for case_name, case in test_cases.items():
            for artifact_id, artifact_case in case.get("artifacts", {}).items():
                if artifact_case.get("file_count", 0) <= 0:
                    continue
                zip_path = data_root / module_name / f"testdata.{module_name}.{artifact_id}.{case_name}.zip"
                if not zip_path.is_file():
                    continue
                expected = latest_expected_result(
                    results_root / module_name,
                    module_name,
                    artifact_id,
                    case_name,
                )
                try:
                    row_count, logs, table = run_fixture(
                        source_root,
                        plugins,
                        module_name,
                        artifact_id,
                        case_name,
                        zip_path,
                        capture,
                    )
                except Exception as error:
                    failures.append(f"{module_name}.{artifact_id}.{case_name}: {type(error).__name__}: {error}")
                    continue
                executed += 1
                parser_errors = [message for level, message in logs if level.casefold() == "error"]
                if parser_errors:
                    failures.append(f"{module_name}.{artifact_id}.{case_name}: {' | '.join(parser_errors)}")
                    continue
                if expected is not None:
                    compared += 1
                    expected_rows = int(expected.get("metadata", {}).get("number_of_rows", 0))
                    if row_count != expected_rows:
                        failures.append(
                            f"{module_name}.{artifact_id}.{case_name}: expected {expected_rows} rows, got {row_count}"
                        )
                elif artifact_case.get("expected_output", {}).get("data") and table is None:
                    failures.append(f"{module_name}.{artifact_id}.{case_name}: emitted no table")

    if failures:
        raise RuntimeError("Fixture failures:\n" + "\n".join(failures))
    return {"executedFixtures": executed, "comparedResults": compared, "failures": 0}


def main() -> int:
    arguments = parse_arguments()
    result = validate_fixtures(arguments.source.resolve(), arguments.plugins.resolve())
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
