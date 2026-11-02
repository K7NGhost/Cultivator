import { artifactModelsByKind } from "../model/artifactModels";
import type { ArtifactModelDefinition, CustomTableColumn, StoredArtifactRecord } from "../model/types";
import { combineCustomTableArtifacts } from "./customTableGrouping";

const provenanceFields = new Set(["deduplication", "group", "raw", "source"]);
const hiddenFields = new Set([...provenanceFields, "kind", "category", "icon", "parser", "provenance"]);
const models: ReadonlyMap<string, ArtifactModelDefinition> = artifactModelsByKind;

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** Present model payloads through the same sortable, source-aware table as plugins. */
export function getArtifactTableView(artifacts: StoredArtifactRecord[]) {
  const custom = combineCustomTableArtifacts(artifacts);
  if (custom) return custom;

  const records = artifacts.filter((artifact) => isObject(artifact.payload));
  if (!records.length) return null;

  const columns = new Map<string, CustomTableColumn>();
  const identityFields = new Set<string>();
  for (const artifact of records) {
    const payload = artifact.payload as Record<string, unknown>;
    const model = models.get(artifact.resultKind);
    for (const field of model?.fields ?? []) {
      if (!hiddenFields.has(field.key) && !columns.has(field.key)) {
        columns.set(field.key, { key: field.key, label: field.label,
          ...(field.type === "datetime" ? { format: "datetime" as const } : {}) });
      }
    }
    for (const key of Object.keys(payload)) {
      if (!provenanceFields.has(key)) identityFields.add(key);
      if (!hiddenFields.has(key) && !columns.has(key)) {
        columns.set(key, { key, label: key.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/_/g, " ") });
      }
    }
  }
  if (!columns.size) return null;

  const name = models.get(records[0].resultKind)?.label ?? records[0].label ?? records[0].resultKind;
  const fields = Array.from(identityFields);
  const tableColumns = Array.from(columns.values());
  return combineCustomTableArtifacts(records.map((artifact) => {
    const payload = artifact.payload as Record<string, unknown>;
    const configuredPolicy = isObject(payload.deduplication) ? payload.deduplication : {};
    const rowFields = new Set(fields);
    if (Array.isArray(configuredPolicy.identityFields)) {
      for (const key of configuredPolicy.identityFields) {
        if (typeof key === "string") rowFields.add(key);
      }
    }
    return {
      ...artifact,
      payload: {
        deduplication: { ...configuredPolicy, identityFields: configuredPolicy.identityFields ?? fields },
        table: {
          name,
          columns: tableColumns,
          // This is a view only: retain the original values and source record.
          rows: [Object.fromEntries(Array.from(rowFields, (key) => [key, payload[key]]))],
        },
      },
    };
  }));
}
