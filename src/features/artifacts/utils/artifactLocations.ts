import type { CustomTableColumn } from "../model/types";
import type { CustomTableView } from "./customTableGrouping";

export type CoordinatePair = { latitude: string; longitude: string; label: string };
export type ArtifactLocationPoint = {
  latitude: number; longitude: number; rowIndex: number; pairLabel: string;
};

function coordinateColumn(column: CustomTableColumn) {
  const candidates = [column.key, column.label].map((value) => value.replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2")
      .replace(/([a-z0-9])([A-Z])/g, "$1 $2").toLowerCase().replace(/^z(latitude|longitude)$/, "$1")
      .split(/[^a-z0-9]+/).filter(Boolean));
  // Scaled coordinates or radians require an explicit conversion upstream.
  if (candidates.some((tokens) => tokens.some((token) => /^(radians?|rad|e[67]|delta|offset|accuracy|uncertainty|deviation|sigma|radius|span)$/.test(token)))) return null;
  for (const tokens of candidates) {
    const index = tokens.findIndex((token) => /^(latitude|lat|longitude|lon|lng|long)$/.test(token));
    if (index < 0) continue;
    const axis = /^(latitude|lat)$/.test(tokens[index]) ? "latitude" as const : "longitude" as const;
    const stem = tokens.filter((token, i) => i !== index && !/^(degrees?|deg|decimal)$/.test(token)).join(" ");
    return { axis, stem };
  }
  return null;
}

export function detectCoordinatePairs(columns: CustomTableColumn[]): CoordinatePair[] {
  const groups = new Map<string, Partial<Record<"latitude" | "longitude", string>>>();
  for (const column of columns) {
    const match = coordinateColumn(column);
    if (!match) continue;
    const group = groups.get(match.stem) ?? {};
    group[match.axis] ??= column.key;
    groups.set(match.stem, group);
  }
  return Array.from(groups, ([label, pair]) => ({ ...pair, label: label || "Location" }))
    .filter((pair): pair is CoordinatePair => Boolean(pair.latitude && pair.longitude));
}

function number(value: unknown): number | null {
  if (typeof value === "string") {
    if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i.test(value.trim())) return null;
    value = Number(value);
  }
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function getArtifactLocations(table: CustomTableView) {
  const pairs = detectCoordinatePairs(table.columns);
  const points: ArtifactLocationPoint[] = [];
  let invalidRows = 0;
  table.rows.forEach((row, rowIndex) => {
    let valid = false;
    for (const pair of pairs) {
      const latitude = number(row.values[pair.latitude]);
      const longitude = number(row.values[pair.longitude]);
      if (latitude === null || longitude === null || Math.abs(latitude) > 90 || Math.abs(longitude) > 180 ||
          (latitude === 0 && longitude === 0)) continue;
      points.push({ latitude, longitude, rowIndex, pairLabel: pair.label });
      valid = true;
    }
    if (!valid) invalidRows++;
  });
  return { points, pairs, invalidRows };
}

/** Smallest longitude span, including evidence on both sides of the date line. */
export function locationBounds(points: ArtifactLocationPoint[]): [[number, number], [number, number]] | null {
  if (!points.length) return null;
  const longitudes = points.map((point) => (point.longitude + 360) % 360).sort((a, b) => a - b);
  let largestGap = -1;
  let start = 0;
  for (let i = 0; i < longitudes.length; i++) {
    const next = i + 1 < longitudes.length ? longitudes[i + 1] : longitudes[0] + 360;
    if (next - longitudes[i] > largestGap) { largestGap = next - longitudes[i]; start = (i + 1) % longitudes.length; }
  }
  let west = longitudes[start];
  let east = west + 360 - largestGap;
  if (west > 180) { west -= 360; east -= 360; }
  let south = 90;
  let north = -90;
  for (const point of points) { south = Math.min(south, point.latitude); north = Math.max(north, point.latitude); }
  return [[west, south], [east, north]];
}
