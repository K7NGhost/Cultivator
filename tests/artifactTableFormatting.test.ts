import { describe, expect, test } from "bun:test";
import { formatArtifactTableCell } from "../src/features/artifacts/utils/artifactTableFormatting";
import { getArtifactTableView } from "../src/features/artifacts/utils/artifactTableView";
import type { CustomTableColumn, StoredArtifactRecord } from "../src/features/artifacts/model/types";

const seconds: CustomTableColumn = { key: "timestamp", label: "Timestamp", format: "datetime", timestampUnit: "seconds" };

describe("artifact timestamp display", () => {
  test("shows typed Unix seconds as readable UTC dates without dropping seconds", () => {
    expect(formatArtifactTableCell(1626796652, seconds)).toBe("2021-07-20 15:57:32 UTC");
    expect(formatArtifactTableCell("1626796652", seconds)).toBe("2021-07-20 15:57:32 UTC");
    expect(formatArtifactTableCell(0, seconds)).toBe("1970-01-01 00:00:00 UTC");
    expect(formatArtifactTableCell(1626796652.125, seconds)).toBe("2021-07-20 15:57:32.125 UTC");
  });

  test("distinguishes explicit millisecond timestamps and ISO date fields", () => {
    expect(formatArtifactTableCell(1626796652000, { ...seconds, timestampUnit: "milliseconds" })).toBe("2021-07-20 15:57:32 UTC");
    expect(formatArtifactTableCell("2021-07-20T17:57:32+02:00", { ...seconds, timestampUnit: undefined })).toBe("2021-07-20 15:57:32 UTC");
  });

  test("preserves date-only and unspecified-zone timestamps", () => {
    expect(formatArtifactTableCell("2021-07-20", seconds)).toBe("2021-07-20");
    expect(formatArtifactTableCell("2021-07-20T15:57:32", seconds)).toBe("2021-07-20T15:57:32");
    expect(formatArtifactTableCell(1626796652, { ...seconds, format: "date" })).toBe("2021-07-20");
  });

  test("does not guess dates from numeric IDs, ambiguous epochs or missing values", () => {
    expect(formatArtifactTableCell(1626796652, { key: "id", label: "ID" })).toBe("1626796652");
    expect(formatArtifactTableCell(1626796652, { key: "time", label: "Time", format: "datetime" })).toBe("1626796652");
    expect(formatArtifactTableCell("not a date", seconds)).toBe("not a date");
    expect(formatArtifactTableCell(1e30, seconds)).toBe("1e+30");
    expect(formatArtifactTableCell(null, seconds)).toBe("");
    expect(formatArtifactTableCell(false, seconds)).toBe("false");
  });

  test("recovers LEAPP date metadata from existing records without reparsing or changing values", () => {
    const artifact: StoredArtifactRecord = { id: "one", resultKind: "custom_table", label: "Log", filePath: "log.db",
      createdAt: "", jobId: "job", pluginId: "ios-artifacts", datasourceId: "source", payload: {
        kind: "custom_table", table: { name: "Log", columns: [{ key: "timestamp", label: "Time" }, { key: "id", label: "ID" }],
          rows: [{ timestamp: 1626796652, id: 1626796652 }] },
        parser: { artifact: { object_columns: [{ name: "timestamp", type: "datetime" }] } },
      } };
    const before = JSON.stringify(artifact);
    const table = getArtifactTableView([artifact])!;
    expect(table.columns[0]).toEqual({ ...seconds, label: "Time" });
    expect(formatArtifactTableCell(table.rows[0].values.timestamp, table.columns[0])).toBe("2021-07-20 15:57:32 UTC");
    expect(formatArtifactTableCell(table.rows[0].values.id, table.columns[1])).toBe("1626796652");
    expect(JSON.stringify(artifact)).toBe(before);
  });
});
