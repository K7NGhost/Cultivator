export const DAY_MS = 86_400_000;
export type Timestamp = { time: number; basis: "zoned" | "unspecified" | "date" };
const validZones = new Map<string, boolean>();

export function isTimeZone(value: string): boolean {
  const cached = validZones.get(value);
  if (cached !== undefined) return cached;
  let valid = false;
  try { new Intl.DateTimeFormat("en", { timeZone: value }); valid = true; } catch { /* Invalid IANA zone. */ }
  validZones.set(value, valid);
  return valid;
}

export function dateStart(value: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const time = Date.parse(`${value}T00:00:00Z`);
  return Number.isFinite(time) && new Date(time).toISOString().slice(0, 10) === value ? time : null;
}

/** Unspecified clocks are encoded without assuming an evidence timezone. */
export function parseTimestamp(value: unknown): Timestamp | null {
  if (typeof value !== "string") return null;
  const text = value.trim();
  const day = dateStart(text);
  if (day !== null) return { time: day, basis: "date" };
  const parts = text.match(/^\d{4}-\d{2}-\d{2}[Tt ](\d{2}):(\d{2})(?::(\d{2})(?:\.\d{1,9})?)?([Zz]|[+-]\d{2}:?\d{2})?$/);
  if (!parts || dateStart(text.slice(0, 10)) === null || +parts[1] > 23 || +parts[2] > 59 || +(parts[3] ?? 0) > 59) return null;
  const basis = parts[4] ? "zoned" : "unspecified";
  const normalized = text.replace(/[Tt ]/, "T").replace(/z$/, "Z");
  const time = Date.parse(basis === "zoned" ? normalized : normalized + "Z");
  return Number.isFinite(time) ? { time, basis } : null;
}

const formatters = new Map<string, Intl.DateTimeFormat>();
/** Calendar coordinates in a zone, represented as milliseconds for comparisons. */
export function wallTime(time: number, timeZone: string): number {
  if (timeZone === "UTC") return time;
  let formatter = formatters.get(timeZone);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat("en-GB", { timeZone, calendar: "iso8601", numberingSystem: "latn",
      year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" });
    formatters.set(timeZone, formatter);
  }
  const parts = Object.fromEntries(formatter.formatToParts(time).map((part) => [part.type, part.value]));
  const milliseconds = ((time % 1000) + 1000) % 1000;
  return Date.parse(`${parts.year.padStart(4, "0")}-${parts.month}-${parts.day}T${parts.hour}:${parts.minute}:${parts.second}Z`) + milliseconds;
}

export function formatInstant(time: number, timeZone: string): string {
  if (!Number.isFinite(time)) return "";
  const local = wallTime(time, timeZone);
  const offset = Math.round((local - time) / 1000);
  const absolute = Math.abs(offset);
  const offsetLabel = `${offset < 0 ? "-" : "+"}${String(Math.floor(absolute / 3600)).padStart(2, "0")}:${String(Math.floor(absolute / 60) % 60).padStart(2, "0")}${absolute % 60 ? `:${String(absolute % 60).padStart(2, "0")}` : ""}`;
  return `${new Date(local).toISOString().replace("T", " ").replace(/\.000Z$/, "Z").replace(/Z$/, "")} ${timeZone}${timeZone === "UTC" ? "" : ` (UTC${offsetLabel})`}`;
}

/** Format only explicit timestamp strings, retaining date-only and unknown clocks. */
export function formatTimestamp(value: string, timeZone: string): string {
  const timestamp = parseTimestamp(value);
  return timestamp?.basis === "zoned" ? formatInstant(timestamp.time, timeZone) : value;
}

/** A display copy, including nested track times; source properties stay intact. */
export function timestampDisplayValue(value: unknown, timeZone: string, depth = 0): unknown {
  if (depth > 32) return value;
  if (typeof value === "string") return formatTimestamp(value, timeZone);
  if (Array.isArray(value)) return value.map((item) => timestampDisplayValue(item, timeZone, depth + 1));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value)
    .map(([key, item]) => [key, timestampDisplayValue(item, timeZone, depth + 1)]));
  return value;
}
