import { expect, test } from "bun:test";
import type { PluginRunSnapshot } from "../src/features/plugins/model/types";
import { waitForPluginRunCompletion } from "../src/features/plugins/utils/waitForPluginRunCompletion";

const finished: PluginRunSnapshot = {
  runId: "run",
  finished: true,
  summary: { datasourceId: "source", datasourcePathsUpdated: true, jobs: [] },
};

test("temporary database locks do not fail a running extraction monitor", async () => {
  const responses = [
    "error returned from database: (code: 5) database is locked",
    new Error("SQLITE_BUSY: writer active"),
    { ...finished, finished: false },
    finished,
  ];
  let reads = 0;
  let waits = 0;
  const summary = await waitForPluginRunCompletion(
    async () => {
      const response = responses[reads++];
      if (typeof response === "string" || response instanceof Error) throw response;
      return response;
    },
    async () => { waits++; },
  );
  expect(summary).toEqual(finished.summary);
  expect(reads).toBe(4);
  expect(waits).toBe(3);
});

test("permanent database errors are reported without retrying", async () => {
  const error = new Error("no such table: plugin_runs");
  let waits = 0;
  await expect(waitForPluginRunCompletion(
    async () => { throw error; },
    async () => { waits++; },
  )).rejects.toBe(error);
  expect(waits).toBe(0);
});

test("completed runs are returned without another poll", async () => {
  let waits = 0;
  const summary = await waitForPluginRunCompletion(
    async () => finished,
    async () => { waits++; },
  );
  expect(summary).toEqual(finished.summary);
  expect(waits).toBe(0);
});
