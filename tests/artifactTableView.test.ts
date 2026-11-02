import { describe, expect, test } from "bun:test";
import { artifactModelDefinitions } from "../src/features/artifacts/model/artifactModels";
import type { StoredArtifactRecord } from "../src/features/artifacts/model/types";
import { getArtifactTableView } from "../src/features/artifacts/utils/artifactTableView";
import { getCustomTableRowSources } from "../src/features/artifacts/utils/customTableGrouping";

function record(id: string, payload: Record<string, unknown>): StoredArtifactRecord {
  return { id, resultKind: String(payload.kind), payload, filePath: `${id}.db`,
    label: "Record", createdAt: "", jobId: "job", pluginId: "plugin", datasourceId: "source" };
}

describe("standard artifact table view", () => {
  test("uses model columns and labels for every standard model", () => {
    for (const model of artifactModelDefinitions.filter((model) => model.kind !== "custom_table")) {
      const table = getArtifactTableView([record(model.kind, { kind: model.kind, label: "Fixture" })]);
      expect(table?.name).toBe(model.label);
      expect(table?.columns).toEqual(model.fields.map(({ key, label, type }) => ({ key, label,
        ...(type === "datetime" ? { format: "datetime" } : {}) })));
      expect(table?.rows[0].values.label).toBe("Fixture");
    }
  });

  test("retains rows, nested values, optional fields, zero and false without mutating records", () => {
    const input = [record("first", { kind: "message", category: "messages", label: "Message", body: "Hello",
      recipients: ["Alice", "Bob"], count: 0, read: false, extraValue: { nested: "value" },
      source: { filePath: "original.db" }, raw: { huge: "raw data" }, parser: { rowOffset: 1 } }),
    record("second", { kind: "message", category: "messages", label: "Other message", body: "World" })];
    const before = JSON.stringify(input);
    const table = getArtifactTableView(input)!;
    expect(table.rows).toHaveLength(2);
    expect(table.rows[0].values).toMatchObject({ body: "Hello", recipients: ["Alice", "Bob"], count: 0,
      read: false, extraValue: { nested: "value" } });
    expect(table.columns.map((column) => column.key)).not.toContain("raw");
    expect(table.columns.map((column) => column.key)).not.toContain("parser");
    expect(table.columns.map((column) => column.key)).not.toContain("source");
    expect(JSON.stringify(input)).toBe(before);
  });

  test("groups duplicate model rows while preserving each source occurrence", () => {
    const payload = { kind: "browser_history", label: "Visit", url: "https://example.com", raw: { row: 1 } };
    const table = getArtifactTableView([record("a", payload), record("b", { ...payload, raw: { row: 2 } })])!;
    expect(table.rows).toHaveLength(1);
    expect(table.totalOccurrences).toBe(2);
    expect(getCustomTableRowSources(table, table.rows[0])).toEqual([
      { artifactId: "a", filePath: "a.db", count: 1 },
      { artifactId: "b", filePath: "b.db", count: 1 },
    ]);
  });

  test("honors preserve and configured identity policies", () => {
    const payload = { kind: "system", label: "Event", key: "Key", value: "Value" };
    const preserve = getArtifactTableView([record("a", { ...payload, deduplication: { mode: "preserve" } }),
      record("b", payload)])!;
    expect(preserve.rows).toHaveLength(2);
    const configured = getArtifactTableView([record("a", { ...payload, deduplication: { identityFields: ["key"] } }),
      record("b", { ...payload, value: "Another value", deduplication: { identityFields: ["key"] } })])!;
    expect(configured.rows).toHaveLength(1);
    expect(configured.totalOccurrences).toBe(2);
  });

  test("leaves custom table columns and values intact", () => {
    const table = getArtifactTableView([record("custom", { kind: "custom_table", table: {
      name: "Plugin table", columns: [{ key: "custom", label: "Custom Column" }], rows: [{ custom: 42 }],
    } })])!;
    expect(table.name).toBe("Plugin table");
    expect(table.columns).toEqual([{ key: "custom", label: "Custom Column" }]);
    expect(table.rows[0].values).toEqual({ custom: 42 });
  });

  test("empty selections have no table", () => {
    expect(getArtifactTableView([])).toBeNull();
  });
});
