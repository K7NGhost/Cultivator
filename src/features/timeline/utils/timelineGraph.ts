import type { TimelineRange } from "../services/timelineRepository";

/** Clamp ApexCharts' continuous zoom coordinates to inclusive database millisecond bounds. */
export function normalizeTimelineZoom(min: number, max: number, first: number, last: number): TimelineRange | null | undefined {
  if (!Number.isFinite(min) || !Number.isFinite(max) || min === max) return undefined;
  const from = Math.max(first, Math.ceil(Math.min(min, max)));
  const to = Math.min(last, Math.floor(Math.max(min, max)));
  if (from > to) return undefined;
  return from === first && to === last ? null : { from, to };
}

export function formatTimelineTick(time: number, includeTime = false): string {
  const iso = new Date(time).toISOString();
  return includeTime ? `${iso.slice(5, 10)} ${iso.slice(11, 19)}` : iso.slice(0, 10);
}

export function formatTimelineBound(time: number): string {
  return new Date(time).toISOString().replace("T", " ").replace("Z", "");
}
