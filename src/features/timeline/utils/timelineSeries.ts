import type { TimelineBucket } from "../services/timelineRepository";

export type TimelineSeriesPoint = { x: number; y: number; bucket: TimelineBucket };

/** Cover the visible window, including the end of its final interval and periods without events. */
export function timelineSeries(buckets: TimelineBucket[], from: number, to: number): TimelineSeriesPoint[] {
  const visible = buckets.filter((bucket) => bucket.end >= from && bucket.start <= to);
  if (!visible.length) return [];
  const points: TimelineSeriesPoint[] = [];
  const add = (x: number, bucket: TimelineBucket) => {
    if (points[points.length - 1]?.x === x) return;
    points.push({ x, y: bucket.count, bucket });
  };
  let cursor = from;
  for (const bucket of visible) {
    const start = Math.max(from, bucket.start);
    if (start > cursor) {
      const empty = { start: cursor, end: start - 1, count: 0 };
      add(cursor, empty);
      add(empty.end, empty);
    }
    add(start, bucket);
    cursor = Math.min(to, bucket.end) + 1;
  }
  const last = visible[visible.length - 1];
  add(Math.min(to, last.end), last);
  if (cursor <= to) {
    const empty = { start: cursor, end: to, count: 0 };
    add(cursor, empty);
    add(to, empty);
  }
  return points;
}
