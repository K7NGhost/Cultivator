import { describe, expect, test } from "bun:test";

import {
  bucketEvents,
  eventsByHour,
  type TimelineEvent,
} from "../src/features/timeline/timelineEvents";

function event(at: Date, type: TimelineEvent["type"] = "message"): TimelineEvent {
  return { id: at.toISOString(), at, type, title: "", detail: "", reveal: { kind: "file", path: "" } };
}

describe("bucketEvents", () => {
  test("uses consecutive days, keeping empty days as gaps", () => {
    const { unit, buckets } = bucketEvents([
      event(new Date(2025, 2, 3, 9)),
      event(new Date(2025, 2, 3, 17), "call"),
      event(new Date(2025, 2, 6, 12), "photo"),
    ]);

    expect(unit).toBe("day");
    expect(buckets.map((bucket) => bucket.total)).toEqual([2, 0, 0, 1]);
    expect(buckets[0].counts).toEqual({ message: 1, call: 1, photo: 0, video: 0 });
  });

  test("switches to months for long spans", () => {
    const { unit, buckets } = bucketEvents([
      event(new Date(2023, 0, 15)),
      event(new Date(2025, 0, 20)),
    ]);

    expect(unit).toBe("month");
    expect(buckets).toHaveLength(25);
    expect(buckets[24].total).toBe(1);
  });
});

test("eventsByHour counts local hours", () => {
  const hours = eventsByHour([event(new Date(2025, 0, 1, 23, 30)), event(new Date(2025, 0, 2, 23, 5))]);

  expect(hours[23]).toBe(2);
  expect(hours.reduce((sum, count) => sum + count, 0)).toBe(2);
});
