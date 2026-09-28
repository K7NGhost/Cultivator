import type { Feature, FeatureCollection, MultiPolygon, Polygon, Position } from "geojson";

import type { MediaItem } from "@/features/media/types";

export type GeoPoint = { lat: number; lon: number };

/**
 * Degrees from an EXIF GPS value, e.g. "37 deg 48 min 28.8 sec N",
 * "122/1 25/1 3372/100 W" or "-122.4177". South and west are negative.
 */
export function parseGpsCoordinate(value: string): number | null {
  const withFractions = value.replace(
    /(\d+(?:\.\d+)?)\s*\/\s*(\d+(?:\.\d+)?)/g,
    (_match, top: string, bottom: string) => String(Number(top) / Number(bottom)),
  );
  const numbers = (withFractions.match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
  if (numbers.length === 0 || numbers.some((number) => !Number.isFinite(number))) {
    return null;
  }

  const [degrees, minutes = 0, seconds = 0] = numbers;
  const magnitude = Math.abs(degrees) + minutes / 60 + seconds / 3600;
  return degrees < 0 || /\b[SW]\b/i.test(value) ? -magnitude : magnitude;
}

/** Where a photo or video was taken, from its EXIF GPS fields. */
export function mediaLocation(item: MediaItem): GeoPoint | null {
  const field = (label: string) => item.metadata.find((entry) => entry.label === label)?.value;
  return validPoint(
    parseGpsCoordinate(field("GPSLatitude") ?? ""),
    parseGpsCoordinate(field("GPSLongitude") ?? ""),
  );
}

/** A usable point, or null for missing, out-of-range or 0,0 ("null island") values. */
export function validPoint(lat: number | null, lon: number | null): GeoPoint | null {
  if (lat === null || lon === null || Math.abs(lat) > 90 || Math.abs(lon) > 180) {
    return null;
  }
  return lat === 0 && lon === 0 ? null : { lat, lon };
}

/**
 * Map coordinates: x = longitude + 180, y = 90 - latitude (equirectangular,
 * the same numbers d3's projection in MapPage produces).
 */
export function project(point: GeoPoint) {
  return { x: point.lon + 180, y: 90 - point.lat };
}

type Topology = {
  transform: { scale: [number, number]; translate: [number, number] };
  arcs: Array<Array<[number, number]>>;
  objects: Record<string, { geometries: Array<{ type: string; arcs?: unknown }> }>;
};

/** A TopoJSON object's polygons as GeoJSON (what topojson-client's `feature` does). */
export function topologyFeatures(topology: Topology, objectName: string): FeatureCollection {
  const { scale, translate } = topology.transform;
  // Arcs are quantized and delta-encoded.
  const arcs = topology.arcs.map((arc) => {
    let x = 0;
    let y = 0;
    return arc.map(([dx, dy]): Position => {
      x += dx;
      y += dy;
      return [x * scale[0] + translate[0], y * scale[1] + translate[1]];
    });
  });
  const ring = (indexes: number[]) => {
    const points: Position[] = [];
    for (const index of indexes) {
      // A negative index is the arc ~index, walked backwards.
      const arc = index >= 0 ? arcs[index] : [...arcs[~index]].reverse();
      points.push(...(points.length > 0 ? arc.slice(1) : arc));
    }
    return points;
  };

  const features = topology.objects[objectName].geometries.flatMap((geometry): Feature[] => {
    if (geometry.type === "Polygon") {
      const coordinates = (geometry.arcs as number[][]).map(ring);
      return [{ type: "Feature", properties: {}, geometry: { type: "Polygon", coordinates } as Polygon }];
    }
    if (geometry.type === "MultiPolygon") {
      const coordinates = (geometry.arcs as number[][][]).map((polygon) => polygon.map(ring));
      return [{ type: "Feature", properties: {}, geometry: { type: "MultiPolygon", coordinates } as MultiPolygon }];
    }
    return [];
  });
  return { type: "FeatureCollection", features };
}

/** Kilometres per degree of longitude at a latitude. */
export function kilometresPerDegree(lat: number) {
  return 111.32 * Math.cos((lat * Math.PI) / 180);
}

/** A round length (1, 2 or 5 times a power of ten) no longer than `limit`. */
export function roundLength(limit: number) {
  const power = 10 ** Math.floor(Math.log10(limit));
  return [5, 2, 1].map((step) => step * power).find((length) => length <= limit) ?? power;
}
