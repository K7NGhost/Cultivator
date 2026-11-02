import type { CustomTableColumn } from "../model/types";

function rawCell(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value !== "object") return String(value);
  try { return JSON.stringify(value); } catch { return String(value); }
}

/** Display typed dates only. Numeric IDs and ambiguous epochs stay untouched. */
export function formatArtifactTableCell(value: unknown, column?: CustomTableColumn): string {
  const raw = rawCell(value);
  if (!column?.format || !raw.trim()) return raw;
  if (typeof value !== "string" && typeof value !== "number") return raw;
  // Date-only and timezone-unspecified values must not shift by a day or zone.
  if (/^\d{4}-\d{2}-\d{2}$/.test(raw)) return raw;
  const numeric = typeof value === "number" || /^[+-]?\d+(?:\.\d+)?$/.test(raw);
  let date: Date;
  if (numeric) {
    if (!column.timestampUnit || !Number.isFinite(Number(value))) return raw;
    date = new Date(Number(value) * (column.timestampUnit === "seconds" ? 1000 : 1));
  } else {
    if (!/^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}.*(?:Z|[+-]\d{2}:?\d{2})$/i.test(raw)) return raw;
    date = new Date(raw);
  }
  if (!Number.isFinite(date.getTime())) return raw;
  const iso = date.toISOString();
  if (column.format === "date") return iso.slice(0, 10);
  return `${iso.replace("T", " ").replace(/\.000Z$/, "Z").replace(/Z$/, "")} UTC`;
}
