import type { FlightRecord } from "../types";

export function elapsedLabel(seconds: number) {
  const total = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(total / 3600)).padStart(2, "0")}:${String(Math.floor(total / 60) % 60).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

export function pointAtTime(points: FlightRecord[], elapsed: number) {
  let low = 0,
    high = points.length - 1;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if ((points[middle].elapsedSeconds ?? 0) <= elapsed) low = middle;
    else high = middle - 1;
  }
  return low;
}

export function finiteLabel(value: number | null | undefined, digits = 1) {
  return value != null && Number.isFinite(value)
    ? value.toLocaleString(undefined, { maximumFractionDigits: digits })
    : "—";
}
