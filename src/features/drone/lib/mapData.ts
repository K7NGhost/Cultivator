import type { FeatureCollection, Point, LineString } from "geojson";
import type { FlightRecord } from "../types";

export type GpsRecord = FlightRecord & { latitude: number; longitude: number };

export function hasGps(record: FlightRecord): record is GpsRecord {
  return (
    record.latitude !== null &&
    record.longitude !== null &&
    Number.isFinite(record.latitude) &&
    Number.isFinite(record.longitude) &&
    Math.abs(record.latitude) <= 90 &&
    Math.abs(record.longitude) <= 180 &&
    (record.latitude !== 0 || record.longitude !== 0)
  );
}

export function mapCoordinates(records: GpsRecord[]): [number, number][] {
  const coordinates: [number, number][] = [];
  for (const record of records) {
    let longitude = record.longitude;
    const previous = coordinates[coordinates.length - 1]?.[0];
    if (previous !== undefined) {
      while (longitude - previous > 180) longitude -= 360;
      while (longitude - previous < -180) longitude += 360;
    }
    coordinates.push([longitude, record.latitude]);
  }
  return coordinates;
}

export function flightGeoJson(records: GpsRecord[]) {
  const coordinates = mapCoordinates(records);
  const route: FeatureCollection<LineString> = {
    type: "FeatureCollection",
    features:
      coordinates.length > 1
        ? [
            {
              type: "Feature",
              properties: {},
              geometry: { type: "LineString", coordinates },
            },
          ]
        : [],
  };
  return { route };
}

export function selectedGeoJson(
  record: FlightRecord,
  anchorLongitude?: number,
): FeatureCollection<Point> {
  if (!hasGps(record)) return { type: "FeatureCollection", features: [] };
  let longitude = record.longitude;
  if (anchorLongitude !== undefined)
    longitude += 360 * Math.round((anchorLongitude - longitude) / 360);
  return {
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        properties: { recordId: record.id },
        geometry: { type: "Point", coordinates: [longitude, record.latitude] },
      },
    ],
  };
}
