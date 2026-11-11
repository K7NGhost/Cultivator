import { DAY_MS, dateStart, parseTimestamp, wallTime, type Timestamp } from "../../../lib/timeZone";
import type { MapFeature, VisibleMapLayer } from "../types";
import { sourceFeatureIndex } from "./mapFeatures";

export type MapDateFilter = { from: string; to: string; fromTime?: string; toTime?: string; includeUndated: boolean };
type TimeRange = { start: number; end: number; basis: Timestamp["basis"] };
const rangesCache = new WeakMap<MapFeature, TimeRange[]>();

function fields(value: Record<string, unknown>): Map<string, unknown> {
  return new Map(Object.entries(value).map(([key, item]) => [key.toLowerCase().replace(/[ _-]/g, ""), item]));
}
function firstTime(values: Map<string, unknown>, keys: string[]): Timestamp | null {
  for (const key of keys) {
    const time = parseTimestamp(values.get(key));
    if (time) return time;
  }
  return null;
}
function rangeOf(times: Timestamp[]): TimeRange[] {
  const ranges = new Map<Timestamp["basis"], TimeRange>();
  for (const { time, basis } of times) {
    const range = ranges.get(basis);
    if (range) { range.start = Math.min(range.start, time); range.end = Math.max(range.end, time); }
    else ranges.set(basis, { start: time, end: time, basis });
  }
  return [...ranges.values()];
}
function featureRanges(feature: MapFeature): TimeRange[] {
  const cached = rangesCache.get(feature);
  if (cached) return cached;
  const properties = feature.properties ?? {};
  const values = fields(properties);
  const instant = firstTime(values, ["time", "timestamp", "datetime", "date", "recordedat", "visitedat"]);
  let result: TimeRange[] = [];
  if (instant) result = rangeOf([instant]);
  else {
    const start = firstTime(values, ["begin", "start", "starttime", "startdate"]);
    const end = firstTime(values, ["end", "stop", "endtime", "enddate"]);
    if (start || end) result = rangeOf([start, end].filter((value): value is Timestamp => value !== null));
    else {
      const times: Timestamp[] = [];
      function collect(value: unknown, depth = 0) {
        if (depth > 32) return;
        if (Array.isArray(value)) { value.forEach((item) => collect(item, depth + 1)); return; }
        const time = parseTimestamp(value);
        if (time) times.push(time);
      }
      collect(values.get("coordinatetimes")); collect(values.get("times"));
      result = rangeOf(times);
      const extended = values.get("extendeddata");
      if (!result.length && extended && typeof extended === "object" && !Array.isArray(extended)) {
        const time = firstTime(fields(extended as Record<string, unknown>), ["time", "timestamp", "datetime", "date", "recordedat"]);
        if (time) result = rangeOf([time]);
      }
    }
  }
  rangesCache.set(feature, result);
  return result;
}

/** Original instants for callers that need the recorded track extent. */
export function featureTimeRange(feature: MapFeature): { start: number; end: number } | null {
  const ranges = featureRanges(feature);
  if (!ranges.length) return null;
  return { start: Math.min(...ranges.map((range) => range.start)), end: Math.max(...ranges.map((range) => range.end)) };
}

function clock(value: string, end: boolean): number | null {
  const parts = value.match(/^(\d{2}):(\d{2})(?::(\d{2})(?:\.(\d{1,3}))?)?$/);
  if (!parts || +parts[1] > 23 || +parts[2] > 59 || +(parts[3] ?? 0) > 59) return null;
  const resolution = parts[4] ? 1 : parts[3] ? 1000 : 60_000;
  return +parts[1] * 3_600_000 + +parts[2] * 60_000 + +(parts[3] ?? 0) * 1000
    + +(parts[4] ?? "0").padEnd(3, "0") + (end ? resolution - 1 : 0);
}

type Window = { from: number; until: number; startTime: number; endTime: number; clockActive: boolean; includeUndated: boolean };
function matchesWallRange(start: number, end: number, basis: Timestamp["basis"], window: Window): boolean {
  start = Math.max(start, window.from); end = Math.min(end, window.until - 1);
  if (start > end) return false;
  if (!window.clockActive) return true;
  if (basis === "date") return window.includeUndated;
  const day = Math.floor(start / DAY_MS) * DAY_MS;
  function intersects(lower: number, upper: number) {
    return Math.max(start, day + lower) <= Math.min(end, day + upper) || day + DAY_MS + lower <= end;
  }
  return window.startTime <= window.endTime ? intersects(window.startTime, window.endTime)
    : intersects(0, window.endTime) || intersects(window.startTime, DAY_MS - 1);
}

function matchesRange(range: TimeRange, window: Window, timeZone: string): boolean {
  if (range.basis !== "zoned" || timeZone === "UTC") return matchesWallRange(range.start, range.end, range.basis, window);
  // Clip near the selected calendar days before considering zone transitions.
  // A week covers the start of a daily window even across skipped local days.
  let cursor = Math.max(range.start, window.from - 2 * DAY_MS);
  const end = Math.min(range.end, window.until + 2 * DAY_MS, cursor + 7 * DAY_MS);
  while (cursor <= end) {
    const next = Math.min(end, cursor + 6 * 3_600_000);
    const offset = wallTime(cursor, timeZone) - cursor;
    const nextOffset = wallTime(next, timeZone) - next;
    if (offset !== nextOffset) {
      // Split at DST changes: repeated clocks match either occurrence, and
      // skipped clocks do not match the nonexistent local hour.
      let low = cursor, high = next;
      while (low + 1 < high) {
        const middle = Math.floor((low + high) / 2);
        if (wallTime(middle, timeZone) - middle === offset) low = middle; else high = middle;
      }
      if (matchesWallRange(cursor + offset, high - 1 + offset, range.basis, window)) return true;
      cursor = high;
    } else {
      if (matchesWallRange(cursor + offset, next + offset, range.basis, window)) return true;
      cursor = next + 1;
    }
  }
  return false;
}

export function filterMapDates(layers: VisibleMapLayer[], filter: MapDateFilter, timeZone = "UTC") {
  const clockActive = !!(filter.fromTime || filter.toTime);
  const active = !!(filter.from || filter.to || clockActive);
  const from = filter.from ? dateStart(filter.from) : -Infinity;
  const to = filter.to ? dateStart(filter.to) : Infinity;
  const startTime = filter.fromTime ? clock(filter.fromTime, false) : 0;
  const endTime = filter.toTime ? clock(filter.toTime, true) : DAY_MS - 1;
  const error = from === null || to === null ? "Enter valid start and end dates."
    : from > to ? "The start date must be on or before the end date."
    : startTime === null || endTime === null ? "Enter valid start and end times." : null;
  const window: Window = { from: from ?? Infinity, until: to === null ? -Infinity : to + DAY_MS,
    startTime: startTime ?? 0, endTime: endTime ?? DAY_MS - 1, clockActive, includeUndated: filter.includeUndated };
  let total = 0, matched = 0, undated = 0;
  const result = layers.map((layer) => {
    const features: MapFeature[] = [];
    const indices: number[] = [];
    layer.collection.features.forEach((feature, index) => {
      total++;
      const ranges = featureRanges(feature);
      if (!ranges.length || (clockActive && ranges.every((range) => range.basis === "date"))) undated++;
      const include = !error && (!active || (ranges.length
        ? ranges.some((range) => matchesRange(range, window, timeZone)) : filter.includeUndated));
      if (include) { features.push(feature); indices.push(sourceFeatureIndex(layer, index)); matched++; }
    });
    if (!active && !error) return layer;
    return { ...layer, totalFeatureCount: layer.totalFeatureCount ?? layer.featureCount, featureCount: features.length,
      featureIndices: indices, collection: { type: "FeatureCollection" as const, features } };
  });
  return { layers: result, total, matched, undated, active, error };
}
