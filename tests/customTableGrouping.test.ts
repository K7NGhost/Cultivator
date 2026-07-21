import { describe, expect, test } from "bun:test";

import {
  combineCustomTableArtifacts,
  getArtifactEntryMetrics,
  getCustomTableRowSources,
} from "../src/features/artifacts/customTableGrouping";
import type { StoredArtifactRecord } from "../src/features/artifacts/types";

function tableArtifact(
  id: string,
  filePath: string,
  rows: Record<string, unknown>[],
  deduplication?: Record<string, unknown>,
): StoredArtifactRecord {
  return {
    id,
    jobId: "job-1",
    pluginId: "test-plugin",
    datasourceId: "datasource-1",
    filePath,
    resultKind: "custom_table",
    label: "Engine Temperature",
    payload: {
      kind: "custom_table",
      category: "vehicles",
      label: "Engine Temperature",
      deduplication,
      table: {
        name: "Engine Temperature",
        columns: [
          { key: "timestamp", label: "Timestamp" },
          { key: "temperature", label: "Temperature" },
        ],
        rows,
      },
    },
    createdAt: "2026-07-21T00:00:00Z",
  };
}

describe("custom table grouping", () => {
  test("groups identical rows while retaining every source occurrence", () => {
    const artifacts = [
      tableArtifact("artifact-1", "archive-a.tar.gz", [
        { timestamp: "2023-06-15T08:05:27Z", temperature: 27 },
      ]),
      tableArtifact("artifact-2", "archive-b.tar.gz", [
        { timestamp: "2023-06-15T08:05:27Z", temperature: 27 },
      ]),
    ];
    const table = combineCustomTableArtifacts(artifacts);

    expect(table?.rows).toHaveLength(1);
    expect(table?.totalOccurrences).toBe(2);
    expect(table?.rows[0].occurrenceCount).toBe(2);
    expect(getCustomTableRowSources(table!, table!.rows[0])).toEqual([
      { artifactId: "artifact-1", count: 1, filePath: "archive-a.tar.gz" },
      { artifactId: "artifact-2", count: 1, filePath: "archive-b.tar.gz" },
    ]);
  });

  test("supports a table-level preserve policy", () => {
    const artifacts = [
      tableArtifact(
        "artifact-1",
        "speed.log",
        [
          { timestamp: "2023-06-15T08:05:27Z", temperature: 27 },
          { timestamp: "2023-06-15T08:05:27Z", temperature: 27 },
        ],
        { mode: "preserve" },
      ),
    ];
    const table = combineCustomTableArtifacts(artifacts);

    expect(table?.rows).toHaveLength(2);
    expect(table?.totalOccurrences).toBe(2);
  });

  test("uses configured identity fields", () => {
    const artifacts = [
      tableArtifact(
        "artifact-1",
        "archive-a.tar.gz",
        [
          { timestamp: "2023-06-15T08:05:27Z", temperature: 27 },
          { timestamp: "2023-06-15T08:05:27Z", temperature: 28 },
        ],
        { mode: "group", identityFields: ["timestamp"] },
      ),
    ];
    const table = combineCustomTableArtifacts(artifacts);

    expect(table?.rows).toHaveLength(1);
    expect(table?.rows[0].occurrenceCount).toBe(2);
  });

  test("falls back to exact rows when table policies conflict", () => {
    const artifacts = [
      tableArtifact(
        "artifact-1",
        "archive-a.tar.gz",
        [{ timestamp: "2023-06-15T08:05:27Z", temperature: 27 }],
        { mode: "group", identityFields: ["timestamp"] },
      ),
      tableArtifact("artifact-2", "archive-b.tar.gz", [
        { timestamp: "2023-06-15T08:05:27Z", temperature: 28 },
      ]),
    ];
    const table = combineCustomTableArtifacts(artifacts);

    expect(table?.rows).toHaveLength(2);
    expect(table?.deduplication.identityFields).toBeUndefined();
  });

  test("groups normalized artifacts without discarding source records", () => {
    const base = tableArtifact("artifact-1", "a.log", []);
    const artifacts: StoredArtifactRecord[] = [
      {
        ...base,
        resultKind: "system",
        payload: {
          kind: "system",
          category: "system",
          label: "VIN",
          key: "VIN",
          value: "ABC123",
          source: { filePath: "a.log" },
        },
      },
      {
        ...base,
        id: "artifact-2",
        filePath: "b.log",
        resultKind: "system",
        payload: {
          kind: "system",
          category: "system",
          label: "VIN",
          key: "VIN",
          value: "ABC123",
          source: { filePath: "b.log" },
        },
      },
    ];

    expect(getArtifactEntryMetrics(artifacts)).toEqual({
      entryCount: 1,
      occurrenceCount: 2,
    });
  });
});
