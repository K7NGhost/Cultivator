"""Inventory every bundled LEAPP artifact and draft mappings without importing parsers.

Suggestions are metadata-only review aids. This tool never changes active rules.
"""
import argparse
import ast
from collections import defaultdict
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
WORKER = ROOT / "src-tauri/src/scripting/worker"
sys.path.insert(0, str(WORKER))
from leapp_normalization import column_key


def headers_in(function):
    schemas = []
    for node in ast.walk(function):
        if not isinstance(node, ast.Assign) or not any(isinstance(target, ast.Name) and "header" in target.id.lower() for target in node.targets):
            continue
        try:
            headers = ast.literal_eval(node.value)
        except (ValueError, TypeError):
            continue
        if isinstance(headers, (list, tuple)):
            labels = [item[0] if isinstance(item, (list, tuple)) else item for item in headers]
            if labels and all(isinstance(label, str) for label in labels) and labels not in schemas:
                schemas.append(labels)
    return schemas


def audit():
    registry = json.loads((WORKER / "leapp_normalization_rules.json").read_text())
    models = json.loads((WORKER / "artifact_model_schemas.json").read_text())["models"]
    rules = {(rule["platform"], rule["artifactKey"]): rule for rule in registry["rules"]}
    aliases = defaultdict(set)
    for rule in registry["rules"]:
        for field, spec in rule["fields"].items():
            aliases[(rule["kind"], field)].update(column_key(label) for label in spec["columns"])
    artifacts = []
    for platform in ("ios", "android", "vehicle"):
        root = ROOT / "src-tauri/resources/mobile" / platform
        catalog = json.loads((root / "catalog.json").read_text())
        for module in catalog["modules"]:
            source = root / module["path"]
            tree = ast.parse(source.read_text(encoding="utf-8-sig"))
            functions = {node.name: node for node in tree.body if isinstance(node, ast.FunctionDef)}
            for artifact in module["artifacts"]:
                key = artifact["key"]
                function = functions.get(artifact.get("function", key))
                schemas = headers_in(function) if function else []
                rule = rules.get((platform, key))
                reviewed = bool(rule and catalog["revision"] == registry["sourceRevisions"].get(platform)
                                and catalog.get("files", {}).get(rule["module"]) == rule["moduleSha256"]
                                and rule["columns"] in schemas)
                entry = {"platform": platform, "project": catalog["project"], "sourceRevision": catalog["revision"],
                         "module": module["path"], "artifactKey": key, "name": artifact.get("name", key),
                         "status": "reviewed" if reviewed else "needs_review", "columns": schemas,
                         "ruleId": rule["id"] if rule else None, "candidates": []}
                if not reviewed and schemas:
                    labels = {column_key(label): label for schema in schemas for label in schema}
                    candidates = []
                    for model in models:
                        if model["kind"] in ("record", "custom_table"):
                            continue
                        fields = {}
                        missing = []
                        for field in model["fields"]:
                            if field["key"] == "label":
                                continue
                            names = aliases[(model["kind"], field["key"])] | {column_key(field["key"])}
                            matches = [label for normalized, label in labels.items() if normalized in names]
                            if len(matches) == 1:
                                fields[field["key"]] = matches[0]
                            elif field.get("required"):
                                missing.append(field["key"])
                        if len(fields) >= 2 and not missing:
                            candidates.append({"kind": model["kind"], "fields": fields, "status": "needs_review",
                                               "note": "Verify semantics, epoch units, timezone and parser variants before enabling."})
                    entry["candidates"] = sorted(candidates, key=lambda item: (-len(item["fields"]), item["kind"]))[:3]
                artifacts.append(entry)
    return {"version": 1, "registryVersion": registry["version"], "summary": {
        "artifacts": len(artifacts), "reviewed": sum(item["status"] == "reviewed" for item in artifacts),
        "needsReview": sum(item["status"] != "reviewed" for item in artifacts),
        "withStaticColumns": sum(bool(item["columns"]) for item in artifacts),
    }, "artifacts": artifacts}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = audit()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"]))
