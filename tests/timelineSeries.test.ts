import { describe, expect, test } from "bun:test";
import { timelineSeries } from "../src/features/timeline/utils/timelineSeries";

describe("zoomed timeline line coverage", () => {
  test("extends the final bucket to its inclusive end instead of stopping at its start", () => {
    const buckets = [{ start: 1000, end: 1999, count: 5 }, { start: 2000, end: 2999, count: 8 }];
    expect(timelineSeries(buckets, 1000, 2999).map(({ x, y }) => [x, y])).toEqual([[1000, 5], [2000, 8], [2999, 8]]);
  });
  test("draws empty time before and after sparse zoom results at zero through both viewport edges", () => {
    const points = timelineSeries([{ start: 2000, end: 2999, count: 5 }], 1000, 9999);
    expect(points.map(({ x, y }) => [x, y])).toEqual([[1000, 0], [1999, 0], [2000, 5], [2999, 5], [3000, 0], [9999, 0]]);
    expect(points.at(-1)?.bucket).toEqual({ start: 3000, end: 9999, count: 0 });
  });
  test("clips coarse fallback data to the viewport while finer zoom data is loading", () => {
    const points = timelineSeries([{ start: 1000, end: 1999, count: 5 }, { start: 2000, end: 2999, count: 8 }], 1500, 2500);
    expect(points.map(({ x, y }) => [x, y])).toEqual([[1500, 5], [2000, 8], [2500, 8]]);
  });
  test("keeps endpoint tooltip values tied to their actual interval", () => {
    const bucket = { start: 1000, end: 1999, count: 8 };
    expect(timelineSeries([bucket], 1000, 1999).map((point) => point.bucket)).toEqual([bucket, bucket]);
  });
  test("preserves empty results and a single instant without inventing activity", () => {
    expect(timelineSeries([], 1000, 1999)).toEqual([]);
    expect(timelineSeries([{ start: 1000, end: 1000, count: 8 }], 1000, 1000)).toHaveLength(1);
    expect(timelineSeries([{ start: 1000, end: 1999, count: 8 }], 2000, 3000)).toEqual([]);
  });
});
