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

/** Map coordinates: x = longitude + 180, y = 90 - latitude (a plain equirectangular map). */
export function project(point: GeoPoint) {
  return { x: point.lon + 180, y: 90 - point.lat };
}

type Topology = {
  transform: { scale: [number, number]; translate: [number, number] };
  arcs: Array<Array<[number, number]>>;
  objects: Record<string, { geometries: Array<{ type: string; arcs?: unknown }> }>;
};

/** SVG path data for a TopoJSON object's polygons, in map coordinates. */
export function topologyPath(topology: Topology, objectName: string): string {
  const { scale, translate } = topology.transform;
  // Arcs are quantized and delta-encoded.
  const arcs = topology.arcs.map((arc) => {
    let x = 0;
    let y = 0;
    return arc.map(([dx, dy]) => {
      x += dx;
      y += dy;
      return [x * scale[0] + translate[0], y * scale[1] + translate[1]] as [number, number];
    });
  });
  const polygons = topology.objects[objectName].geometries.flatMap((geometry) =>
    geometry.type === "Polygon"
      ? [geometry.arcs as number[][]]
      : geometry.type === "MultiPolygon"
        ? (geometry.arcs as number[][][])
        : [],
  );

  return polygons
    .flatMap((rings) =>
      rings.map((ring) => {
        const points: Array<[number, number]> = [];
        for (const index of ring) {
          // A negative index is the arc ~index, walked backwards.
          const arc = index >= 0 ? arcs[index] : [...arcs[~index]].reverse();
          points.push(...(points.length > 0 ? arc.slice(1) : arc));
        }
        return `M${points
          .map(([lon, lat]) => `${(lon + 180).toFixed(3)},${(90 - lat).toFixed(3)}`)
          .join("L")}Z`;
      }),
    )
    .join("");
}
