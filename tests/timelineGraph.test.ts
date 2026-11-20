import { describe, expect, test } from "bun:test";
import { formatTimelineBound, formatTimelineTick, normalizeTimelineZoom } from "../src/features/timeline/utils/timelineGraph";

describe("timeline line graph zoom", () => {
  test("converts fractional chart coordinates into inclusive database millisecond bounds", () => {
    expect(normalizeTimelineZoom(1100.2, 2700.8, 1000, 3999)).toEqual({ from: 1101, to: 2700 });
    expect(normalizeTimelineZoom(1100, 2700, 1000, 3999)).toEqual({ from: 1100, to: 2700 });
  });
  test("supports reverse drags and clamps zoom-out to the full event extent", () => {
    expect(normalizeTimelineZoom(2700, 1100, 1000, 3999)).toEqual({ from: 1100, to: 2700 });
    expect(normalizeTimelineZoom(500, 2700, 1000, 3999)).toEqual({ from: 1000, to: 2700 });
    expect(normalizeTimelineZoom(1100, 5000, 1000, 3999)).toEqual({ from: 1100, to: 3999 });
    expect(normalizeTimelineZoom(0, 5000, 1000, 3999)).toBeNull();
  });
  test("ignores invalid or empty gestures without resetting an existing zoom", () => {
    for (const [min, max] of [[NaN, 2000], [1000, Infinity], [2000, 2000], [0, 1], [2000.1, 2000.2]]) {
      expect(normalizeTimelineZoom(min, max, 1000, 3999)).toBeUndefined();
    }
  });
  test("preserves pre-epoch timestamps and supports a single instant", () => {
    expect(normalizeTimelineZoom(-100.2, 100.2, -1000, 1000)).toEqual({ from: -100, to: 100 });
    expect(normalizeTimelineZoom(0, 2000, 1000, 1000)).toBeNull();
  });
  test("labels exact selection boundaries independently of machine timezone", () => {
    const time = Date.parse("2026-11-19T01:02:03.123Z");
    expect(formatTimelineTick(time)).toBe("2026-11-19");
    expect(formatTimelineTick(time, true)).toBe("11-19 01:02:03");
    expect(formatTimelineBound(time)).toBe("2026-11-19 01:02:03.123");
  });
});
