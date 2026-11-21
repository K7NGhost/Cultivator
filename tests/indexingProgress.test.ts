import { expect, test } from "bun:test";
import { indexingProgress } from "../src/features/settings/indexingProgress";

test("background checks preserve saved coverage instead of appearing to rebuild", () => {
  const view = indexingProgress({ name: "Artifacts", processed: 130, total: 130, skipped: 2, state: "Checking", checked: 32 });
  expect(view.value).toBe(100);
  expect(view.summary).toContain("32 checked");
  expect(view.summary).toContain("saved coverage");
  expect(view.summary).toContain("130 / 130 (100%)");
  expect(view.summary).toContain("2 excluded");
  expect(indexingProgress({ name: "Artifacts", processed: 0, total: 0, skipped: 0, state: "Checking", checked: 0 }).value).toBe(100);
});

test("initial indexing still shows real progress and unknown discovery totals", () => {
  expect(indexingProgress({ name: "Files", processed: 25, total: 100, skipped: 0, state: "Indexing" }).value).toBe(25);
  expect(indexingProgress({ name: "Metadata", processed: 32, total: 0, skipped: 0, state: "Discovering" }).value).toBeUndefined();
  expect(indexingProgress({ name: "Files", processed: 0, total: 0, skipped: 0, state: "Waiting" }).value).toBe(0);
});
