import type { IndexedHit } from "../services/indexedSearchRepository";
import { formatInstant } from "@/lib/timeZone";

export interface IndexedField { label: string; value: string }
export interface IndexedRecordDisplay { title: string; fields: IndexedField[]; text: string; partial: boolean; summary: string }
const acronyms = new Map([["id", "ID"], ["uid", "UID"], ["gid", "GID"], ["url", "URL"], ["readonly", "Read only"]]);

/** Preserve integer identifiers that exceed JavaScript's exact number range. */
function parseJson(text: string): unknown {
  return JSON.parse(text.replace(/"(?:[^"\\]|\\.)*"|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g, (token, numeric: string | undefined) =>
    numeric && /^-?\d+$/.test(numeric) && !Number.isSafeInteger(Number(numeric)) ? JSON.stringify(numeric) : token));
}

function label(key: string): string {
  const words = key.replace(/([a-z0-9])([A-Z])/g, "$1 $2").replace(/[_-]+/g, " ").trim();
  return words.split(/\s+/).map(word => acronyms.get(word.toLowerCase()) ?? (word ? word[0].toUpperCase() + word.slice(1) : "Field")).join(" ");
}

function scalar(value: unknown, key: string, metadata: boolean, timeZone: string): string {
  if (value === null) return "No value recorded";
  if (value === true) return "Yes";
  if (value === false) return "No";
  if (value === "") return "Empty";
  if (metadata && ["size", "bytesize", "allocatedsize"].includes(key) && typeof value === "number" && value >= 0) {
    const bytes = `${value.toLocaleString("en-US")} bytes`;
    const unit = Math.min(4, Math.floor(Math.log2(Math.max(1, value)) / 10));
    return unit ? `${Number((value / 1024 ** unit).toFixed(2))} ${["B", "KiB", "MiB", "GiB", "TiB"][unit]} (${bytes})` : bytes;
  }
  // Local filesystem metadata explicitly stores nanoseconds as decimal strings.
  // Do not guess units for numeric artifact IDs or messaging timestamps.
  if (metadata && ["created", "modified"].includes(key) && typeof value === "string" && /^\d{16,21}$/.test(value)) {
    const time = Number(BigInt(value) / BigInt(1_000_000));
    if (time <= 8.64e15) return formatInstant(time, timeZone);
  }
  return String(value);
}

function flatten(value: unknown, path: string[], fields: IndexedField[], metadata: boolean, timeZone: string, depth = 0): boolean {
  if (fields.length >= 200 || depth > 12) return true;
  if (value && typeof value === "object") {
    const object = value as Record<string, unknown>;
    if (metadata && typeof object.seconds === "number" && typeof object.nanos === "number" && ["utc", "local_civil"].includes(String(object.basis))) {
      const time = object.seconds * 1000 + Math.floor(object.nanos / 1_000_000);
      if (Number.isFinite(time) && Math.abs(time) <= 8.64e15) {
        const date = object.basis === "utc" ? formatInstant(time, timeZone) : `${new Date(time).toISOString().replace("T", " ").replace(/Z$/, "")} (recorded local time)`;
        fields.push({ label: path.join(" › "), value: date });
        return false;
      }
    }
    const entries = Object.entries(object);
    if (!entries.length) fields.push({ label: path.join(" › ") || "Record", value: Array.isArray(value) ? "None" : "No fields recorded" });
    let omitted = false;
    for (const [key, child] of entries) omitted = flatten(child, [...path, Array.isArray(value) ? `Item ${Number(key) + 1}` : label(key)], fields, metadata, timeZone, depth + 1) || omitted;
    return omitted;
  }
  fields.push({ label: path.join(" › ") || "Value", value: scalar(value, path[path.length - 1]?.replace(/\s/g, "").toLowerCase() ?? "", metadata, timeZone) });
  return false;
}

function decodeFragment(raw: string): string {
  // Chunks can end partway through a JSON escape. Trim only that unfinished
  // suffix, and retain the exact source in the collapsed original-text view.
  for (let trim = 0; trim <= Math.min(6, raw.length); trim++) {
    try { return JSON.parse(`"${raw.slice(0, raw.length - trim)}"`) as string; } catch { /* Try an incomplete trailing escape. */ }
  }
  return raw;
}

function partialFields(body: string, metadata: boolean, timeZone: string): { fields: IndexedField[]; text: string } {
  const fields: IndexedField[] = [];
  // Recover only explicit key/value pairs. No evaluation or synthetic values.
  const keys = /(?:^|[,{])\s*("(?:[^"\\]|\\.)*")\s*:\s*/g;
  let match: RegExpExecArray | null;
  let first = -1;
  while ((match = keys.exec(body)) && fields.length < 200) {
    if (first < 0) first = match.index;
    let key: string;
    try { key = JSON.parse(match[1]) as string; } catch { continue; }
    const start = keys.lastIndex;
    if (body[start] === '"') {
      let end = start + 1;
      for (; end < body.length; end++) {
        if (body[end] === "\\") { end++; continue; }
        if (body[end] === '"') break;
      }
      const raw = body.slice(start + 1, Math.min(end, body.length));
      fields.push({ label: label(key), value: decodeFragment(raw) + (end >= body.length ? "…" : "") });
      keys.lastIndex = Math.min(end + 1, body.length);
    } else {
      const primitive = body.slice(start).match(/^(?:true|false|null|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?)(?=\s*[,}\]]|$)/);
      if (primitive) fields.push({ label: label(key), value: scalar(parseJson(primitive[0]), key.toLowerCase(), metadata, timeZone) });
    }
  }
  const leading = first > 0 ? body.slice(0, first).trim() : first < 0 ? body.trim() : "";
  const text = leading && !/^[\[{]/.test(leading) ? decodeFragment(leading.replace(/"$/, "")) : "";
  return { fields, text };
}

export function indexedRecordDisplay(hit: IndexedHit, timeZone = "UTC"): IndexedRecordDisplay {
  const parts = hit.source.split(/[\\/]/).filter(Boolean);
  const filename = parts[parts.length - 1] || "Indexed record";
  let title = filename;
  let body = hit.body.trim();
  const fields: IndexedField[] = [];
  if (hit.stage === "2") return { title, fields, text: hit.body, partial: false, summary: hit.body.slice(0, 500) };
  if (!/^[\[{]/.test(body)) {
    const newline = body.indexOf("\n");
    if (hit.chunk === 0 && newline >= 0 && /^[\[{]/.test(body.slice(newline + 1).trimStart())) {
      if (hit.stage === "1") title = body.slice(0, newline).trim() || filename;
      body = body.slice(newline + 1).trim();
    }
  }
  let partial = false;
  let text = "";
  try {
    const value = parseJson(body);
    partial = flatten(value, [], fields, hit.stage === "0", timeZone);
    if (partial && !fields.length) text = "Nested data is available in the original indexed text.";
  } catch {
    const recovered = partialFields(body, hit.stage === "0", timeZone);
    fields.push(...recovered.fields); text = recovered.text;
    partial = fields.length > 0 || hit.chunk > 0 || /^[\[{]/.test(body);
    if (!fields.length && !text) text = hit.body;
  }
  const priority = (field: IndexedField) => /(?:^| › )(?:message|body|text|content|description)$/i.test(field.label) ? 0 : /sender|receiver|participant|name|title|time|date|modified|created|size/i.test(field.label) ? 1 : 2;
  const summary = [...fields].sort((a, b) => priority(a) - priority(b)).slice(0, 4).map(field => `${field.label}: ${field.value}`).join(" · ");
  return { title, fields, text, partial, summary: (summary || text).slice(0, 500) };
}
