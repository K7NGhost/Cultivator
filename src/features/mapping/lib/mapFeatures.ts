import type { Geometry } from "geojson";
import type { MapFeature, MapLayerData, VisibleMapLayer } from "../types";

export const LAYER_COLORS = ["#f97316", "#0d9488", "#3b82f6", "#a855f7", "#e11d48", "#ca8a04"];

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function coordinates(value: unknown, depth: number): boolean {
  if (!Array.isArray(value)) return false;
  if (depth > 0) return value.every((item) => coordinates(item, depth - 1));
  return value.length >= 2 && value.length <= 3 && value.every((n) => typeof n === "number" && Number.isFinite(n))
    && Math.abs(value[0]) <= 180 && Math.abs(value[1]) <= 90;
}

function geometry(value: unknown, depth = 0): value is Geometry {
  if (!object(value) || depth > 32) return false;
  if (value.type === "GeometryCollection") return Array.isArray(value.geometries) && value.geometries.every((g) => geometry(g, depth + 1));
  switch (value.type) {
    case "Point": return coordinates(value.coordinates, 0);
    case "MultiPoint": case "LineString": return coordinates(value.coordinates, 1);
    case "MultiLineString": case "Polygon": return coordinates(value.coordinates, 2);
    case "MultiPolygon": return coordinates(value.coordinates, 3);
    default: return false;
  }
}

function feature(value: unknown): value is MapFeature {
  return object(value) && value.type === "Feature" && geometry(value.geometry)
    && (value.properties === null || object(value.properties));
}

export function parseMapLayer(payload: string): MapLayerData {
  const value: unknown = JSON.parse(payload);
  if (!object(value) || value.kind !== "map_layer" || !object(value.featureCollection)
    || value.featureCollection.type !== "FeatureCollection" || !Array.isArray(value.featureCollection.features)
    || value.featureCollection.features.length > 100_000 || !value.featureCollection.features.every(feature)) {
    throw new Error("This map layer contains invalid geometry. Run Map Files again to regenerate it.");
  }
  return { collection: { type: "FeatureCollection", features: value.featureCollection.features },
    warnings: Array.isArray(value.warnings) ? value.warnings.filter((warning): warning is string => typeof warning === "string") : [] };
}

export function featureName(feature: MapFeature, index: number): string {
  const properties = feature.properties;
  for (const field of ["name", "title", "description", "desc"]) {
    const value: unknown = properties?.[field];
    if (typeof value === "string" && value.trim()) return value;
  }
  return `${feature.geometry.type} ${index + 1}`;
}

export function featureKey(layerId: string, index: number) { return `${layerId}:${index}`; }

export function sourceFeatureIndex(layer: VisibleMapLayer, index: number) {
  return layer.featureIndices?.[index] ?? index;
}

export function featureAtSourceIndex(layer: VisibleMapLayer, sourceIndex: number): MapFeature | undefined {
  const index = layer.featureIndices ? layer.featureIndices.indexOf(sourceIndex) : sourceIndex;
  return layer.collection.features[index];
}

export function renderGeometry(geometry: Geometry): Geometry {
  const line = (positions: GeoJSON.Position[], anchor = positions[0]?.[0]) => {
    let previous = anchor;
    return positions.map((position) => {
      let longitude = position[0];
      if (previous !== undefined) longitude += 360 * Math.round((previous - longitude) / 360);
      previous = longitude;
      return [longitude, ...position.slice(1)];
    });
  };
  const polygon = (rings: GeoJSON.Position[][]) => rings.map((ring) => line(ring, rings[0]?.[0]?.[0]));
  switch (geometry.type) {
    case "LineString": return { ...geometry, coordinates: line(geometry.coordinates) };
    case "MultiLineString": return { ...geometry, coordinates: geometry.coordinates.map((points) => line(points, geometry.coordinates[0]?.[0]?.[0])) };
    case "Polygon": return { ...geometry, coordinates: polygon(geometry.coordinates) };
    case "MultiPolygon": return { ...geometry, coordinates: geometry.coordinates.map(polygon) };
    case "GeometryCollection": return { ...geometry, geometries: geometry.geometries.map(renderGeometry) };
    default: return geometry;
  }
}

export function renderCollection(layers: VisibleMapLayer[]): GeoJSON.FeatureCollection {
  return { type: "FeatureCollection", features: layers.flatMap((layer) => layer.collection.features.map((item, index) => ({
    ...item, id: featureKey(layer.id, sourceFeatureIndex(layer, index)), geometry: renderGeometry(item.geometry),
    properties: { ...item.properties, _cvLayer: layer.id, _cvIndex: sourceFeatureIndex(layer, index),
      _cvKey: featureKey(layer.id, sourceFeatureIndex(layer, index)), _cvColor: layer.color },
  }))) };
}

/** Fit the shortest longitude interval, including layers crossing the date line. */
export function geometryBounds(geometries: Geometry[]): [[number, number], [number, number]] | null {
  const longitudes: number[] = [];
  let south = Infinity, north = -Infinity;
  function visit(geometry: Geometry) {
    if (geometry.type === "GeometryCollection") { geometry.geometries.forEach(visit); return; }
    function walk(value: unknown) {
      if (!Array.isArray(value)) return;
      if (typeof value[0] === "number" && typeof value[1] === "number") {
        longitudes.push(((value[0] % 360) + 360) % 360);
        south = Math.min(south, value[1]); north = Math.max(north, value[1]);
      } else value.forEach(walk);
    }
    walk(geometry.coordinates);
  }
  geometries.forEach(visit);
  if (!longitudes.length) return null;
  longitudes.sort((a, b) => a - b);
  let gap = -1, start = 0;
  for (let i = 0; i < longitudes.length; i++) {
    const next = i + 1 === longitudes.length ? longitudes[0] + 360 : longitudes[i + 1];
    if (next - longitudes[i] > gap) { gap = next - longitudes[i]; start = (i + 1) % longitudes.length; }
  }
  let west = longitudes[start], east = west + (360 - gap);
  if (west > 180) { west -= 360; east -= 360; }
  return [[west, south], [east, north]];
}
