"""Reviewed LEAPP row mappings. No parser imports, inference, or evidence writes."""
from datetime import datetime, timedelta, timezone
from decimal import Decimal, InvalidOperation
import json
import math
import re


def column_key(value):
    return re.sub(r"[^a-z0-9]+", "", str(value).lower())


def convert_value(value, conversion, object_type=None):
    if value is None or value == "":
        return None
    if conversion == "string":
        if isinstance(value, (dict, list, bytes, bool)):
            raise ValueError("expected scalar text")
        return str(value)
    if conversion == "strings":
        if isinstance(value, str) and value.lstrip().startswith("["):
            value = json.loads(value)
        values = value if isinstance(value, list) else [value]
        return [convert_value(item, "string") for item in values if item is not None and item != ""]
    if conversion in ("number", "integer", "milliseconds_to_seconds"):
        if isinstance(value, bool):
            raise ValueError("boolean is not a number")
        try:
            number = Decimal(str(value))
        except InvalidOperation as error:
            raise ValueError("invalid number") from error
        if not number.is_finite():
            raise ValueError("non-finite number")
        if conversion == "integer":
            if number != number.to_integral_value():
                raise ValueError("expected integer")
            return int(number)
        result = float(number / 1000 if conversion == "milliseconds_to_seconds" else number)
        if not math.isfinite(result):
            raise ValueError("number outside supported range")
        return result
    if conversion == "boolean":
        if isinstance(value, bool):
            return value
        lookup = {"true": True, "false": False, "1": True, "0": False, "yes": True, "no": False}
        if str(value).lower() not in lookup:
            raise ValueError("invalid boolean")
        return lookup[str(value).lower()]
    if conversion == "datetime":
        # LAVA explicitly tags datetime columns and stores them as UTC seconds.
        # Untyped numeric dates are ambiguous and must never be guessed.
        if object_type == "datetime":
            return convert_value(value, "unix_seconds")
        if not isinstance(value, str) or not re.match(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}", value):
            raise ValueError("expected ISO datetime or a typed LAVA datetime")
        return datetime.fromisoformat(value.strip().replace("Z", "+00:00")).isoformat()
    if conversion in ("unix_seconds", "unix_milliseconds", "unix_microseconds", "webkit_microseconds", "cocoa_seconds"):
        if isinstance(value, bool):
            raise ValueError("invalid epoch")
        number = Decimal(str(value))
        if not number.is_finite():
            raise ValueError("non-finite epoch")
        divisor = 1000 if conversion == "unix_milliseconds" else 1000000 if conversion in ("unix_microseconds", "webkit_microseconds") else 1
        origin = datetime(1601, 1, 1, tzinfo=timezone.utc) if conversion == "webkit_microseconds" else datetime(2001, 1, 1, tzinfo=timezone.utc) if conversion == "cocoa_seconds" else datetime(1970, 1, 1, tzinfo=timezone.utc)
        return (origin + timedelta(seconds=float(number / divisor))).isoformat()
    if conversion == "json":
        return json.loads(value) if isinstance(value, str) else value
    raise ValueError(f"unknown conversion: {conversion}")


def validate_payload(payload, schema):
    for field in schema["fields"]:
        value = payload.get(field["key"])
        if value is None:
            if field.get("required"):
                raise ValueError(f"missing required field: {field['key']}")
            continue
        kind = field["type"]
        valid = (kind in ("string", "datetime") and isinstance(value, str)) or (
            kind == "number" and isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)
        ) or (kind == "boolean" and isinstance(value, bool)) or (
            kind == "string[]" and isinstance(value, list) and all(isinstance(item, str) for item in value)
        ) or kind == "json"
        if not valid:
            raise ValueError(f"invalid field type: {field['key']}")
        if field.get("required") and isinstance(value, str) and not value.strip():
            raise ValueError(f"empty required field: {field['key']}")
    for key, low, high in (("latitude", -90, 90), ("longitude", -180, 180)):
        if key in payload and not low <= payload[key] <= high:
            raise ValueError(f"invalid coordinate: {key}")
    for key in ("size", "durationSeconds", "visitCount", "pageCount", "distanceMeters", "accuracyMeters"):
        if key in payload and payload[key] < 0:
            raise ValueError(f"negative measurement: {key}")


class LeappNormalizer:
    def __init__(self, registry, schemas, enabled=True):
        if registry.get("version") != 1 or schemas.get("version") != 1:
            raise ValueError("unsupported normalization registry/schema version")
        self.registry = registry
        self.schemas = {model["kind"]: model for model in schemas["models"]}
        self.enabled = enabled
        self.rules = {}
        for rule in registry["rules"]:
            if not rule.get("reviewed") or rule["kind"] not in self.schemas:
                raise ValueError("normalization rules must be reviewed and reference a known model")
            fields = {field["key"] for field in self.schemas[rule["kind"]]["fields"]}
            if not set(rule["fields"]) <= fields or not set(rule.get("constants", {})) <= fields:
                raise ValueError(f"unknown model field in rule {rule['id']}")
            identity = (rule["platform"], rule["artifactKey"])
            if identity in self.rules:
                raise ValueError(f"duplicate mapping: {identity}")
            self.rules[identity] = rule

    def plan(self, catalog, artifact, columns):
        rule = self.rules.get((catalog["platform"], artifact.get("artifact_key")))
        status = "disabled" if not self.enabled else "unmapped" if not rule else "mapped"
        if status == "mapped" and self.registry["sourceRevisions"].get(catalog["platform"]) != catalog["revision"]:
            status = "revision_mismatch"
        if status == "mapped" and catalog.get("files", {}).get(rule["module"]) != rule["moduleSha256"]:
            status = "source_mismatch"
        by_label = {}
        for column in columns:
            for label in {column["key"], column["label"]}:
                keys = by_label.setdefault(column_key(label), set())
                keys.add(column["key"])
        bindings = {}
        conditions = {}
        if status == "mapped":
            if any(len(by_label.get(column_key(label), ())) != 1 for label in rule["columns"]):
                status = "schema_mismatch"
            else:
                for field, spec in rule["fields"].items():
                    matches = set().union(*(by_label.get(column_key(label), set()) for label in spec["columns"]))
                    if len(matches) > 1:
                        status = "schema_mismatch"
                    elif matches:
                        bindings[field] = next(iter(matches))
                    if spec.get("when"):
                        keys = by_label.get(column_key(spec["when"]["column"]), set())
                        if len(keys) != 1:
                            status = "schema_mismatch"
                        else:
                            conditions[field] = {"key": next(iter(keys)), "values": spec["when"]["values"]}
        consumed = set(bindings.values()) | {condition["key"] for condition in conditions.values()}
        return {"rule": rule, "bindings": bindings, "conditions": conditions, "status": status, "artifactKey": artifact.get("artifact_key"),
                "module": artifact.get("module"), "table": artifact["tablename"], "model": rule["kind"] if rule else None,
                "ruleId": rule["id"] if rule else None, "rows": 0, "normalizedRows": 0, "fallbackRows": 0,
                "invalidRows": 0, "unmappedColumns": [column["key"] for column in columns if column["key"] not in consumed], "errors": []}

    def normalize(self, plan, row, base, row_index, artifact):
        plan["rows"] += 1
        if plan["status"] != "mapped":
            plan["fallbackRows"] += 1
            return None
        rule = plan["rule"]
        types = {column["name"]: column["type"] for column in artifact.get("object_columns", [])}
        schema = self.schemas[rule["kind"]]
        payload = {key: value for key, value in base.items() if key != "table"}
        payload.update(kind=rule["kind"], category=schema["category"])
        payload.update(rule.get("constants", {}))
        try:
            for field, key in plan["bindings"].items():
                condition = plan["conditions"].get(field)
                if condition and row.get(condition["key"]) not in condition["values"]:
                    continue
                spec = rule["fields"][field]
                value = None if row[key] in spec.get("nullValues", []) else convert_value(row[key], spec["convert"], types.get(key))
                if value is not None:
                    payload[field] = value
            validate_payload(payload, schema)
        except (ValueError, TypeError, OverflowError, KeyError, InvalidOperation) as error:
            plan["fallbackRows"] += 1
            plan["invalidRows"] += 1
            if len(plan["errors"]) < 20:
                plan["errors"].append({"rowIndex": row_index, "error": str(error)})
            return None
        payload["parser"] = {**base["parser"], "rowOffset": row_index}
        payload["raw"] = {"columns": base["table"]["columns"], "row": row}
        payload["normalization"] = {"version": 1, "ruleId": rule["id"], "schemaVersion": schema["schemaVersion"],
                                    "sourceRevision": base["parser"]["revision"]}
        # Record how timestamp conversion was chosen, including unspecified zones.
        payload["timestamps"] = [{"label": field, "value": payload[field],
            "timezone": "unspecified" if datetime.fromisoformat(payload[field]).tzinfo is None else datetime.fromisoformat(payload[field]).strftime("%z"),
            "source": f"{key} ({types.get(key) or rule['fields'][field]['convert']})"}
            for field, key in plan["bindings"].items()
            if field in payload and next(item for item in schema["fields"] if item["key"] == field)["type"] == "datetime"]
        plan["normalizedRows"] += 1
        return payload


def normalization_report(plans):
    public = [{key: value for key, value in plan.items() if key not in ("rule", "bindings", "conditions")} for plan in plans]
    return {"version": 1, "tables": public, "totals": {
        key: sum(plan[key] for plan in plans) for key in ("rows", "normalizedRows", "fallbackRows", "invalidRows")}}
