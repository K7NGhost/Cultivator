import { describe, expect, test } from "bun:test";
import { getArtifactEntryMetrics } from "../src/features/artifacts/utils/customTableGrouping";
import type { StoredArtifactRecord } from "../src/features/artifacts/model/types";

describe("artifact catalog memory", () => {
  test("counts catalog records without reading or grouping their payloads", () => {
    const artifact={id:"one",jobId:"job",pluginId:"parser",datasourceId:"source",filePath:"/test.db",resultKind:"custom_table",label:"Large table",createdAt:"",summaryOnly:true,entryCount:1_000_000,payloadBytes:1_000_000_000} as StoredArtifactRecord;
    Object.defineProperty(artifact,"payload",{get(){throw new Error("The sidebar must not load, hash, or serialize row payloads");}});
    const second=Object.assign(Object.create(artifact),{id:"two",entryCount:12}) as StoredArtifactRecord;
    expect(getArtifactEntryMetrics([artifact,second])).toEqual({entryCount:1_000_012,occurrenceCount:1_000_012});
    expect(getArtifactEntryMetrics([])).toEqual({entryCount:0,occurrenceCount:0});
  });
});
