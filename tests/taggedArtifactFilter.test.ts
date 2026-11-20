import { describe, expect, test } from "bun:test";
import { filterTaggedArtifacts } from "../src/features/artifacts/utils/taggedArtifactFilter";
import type { StoredArtifactRecord } from "../src/features/artifacts/model/types";

const record = (id: string, datasourceId: string): StoredArtifactRecord => ({ id, datasourceId, jobId: "job", pluginId: "parser", filePath: `/${id}.db`, resultKind: "custom_table", label: id, payload: { table: { name: id } }, createdAt: "2026-11-20", summaryOnly: true, entryCount: 250 });

describe("tagged artifact explorer filter", () => {
  test("limits groups and counts to tagged occurrences without altering saved catalog records", () => {
    const artifacts = [record("tagged", "phone"), record("untagged", "phone"), record("other-source", "drive")];
    const counts = new Map([["tagged", 2], ["other-source", 1], ["stale-id", 99]]);
    const visible = filterTaggedArtifacts(artifacts.filter((item) => item.datasourceId === "phone"), true, counts);
    expect(visible.map((item) => [item.id, item.entryCount])).toEqual([["tagged", 2]]);
    expect(visible[0].filePath).toBe("/tagged.db");
    expect(artifacts[0].entryCount).toBe(250);
    expect(visible[0].payload).toBe(artifacts[0].payload);
  });
  test("restores all records when cleared and yields no matches for a case with no tags", () => {
    const artifacts = [record("one", "phone")];
    expect(filterTaggedArtifacts(artifacts, false, new Map())).toBe(artifacts);
    expect(filterTaggedArtifacts(artifacts, true, new Map())).toEqual([]);
  });
});
