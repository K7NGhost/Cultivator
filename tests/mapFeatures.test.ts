import { describe, expect, test } from "bun:test";
import { featureName, geometryBounds, parseMapLayer, renderCollection, renderGeometry } from "../src/features/mapping/lib/mapFeatures";
import type { VisibleMapLayer } from "../src/features/mapping/types";

function layer(): VisibleMapLayer {
  return { id: "layer", label: "Places", filePath: "/places.geojson", datasourceId: "source", format: "geojson",
    featureCount: 1, positionCount: 1, color: "#f97316", warnings: [],
    collection: { type: "FeatureCollection", features: [{ type: "Feature", id: "original-id",
      properties: { name: "<b>Literal name</b>", owner: { name: "Alice" } }, geometry: { type: "Point", coordinates: [-73, 40, 12] } }] } };
}

describe("map file layers", () => {
  test("loads original feature properties and discards non-string warnings", () => {
    const value = layer();
    const parsed = parseMapLayer(JSON.stringify({ kind: "map_layer", featureCollection: value.collection, warnings: ["Missing timestamp", 1] }));
    expect(parsed.collection).toEqual(value.collection);
    expect(parsed.warnings).toEqual(["Missing timestamp"]);
  });
  test("rejects corrupted stored geometry rather than passing it to WebGL", () => {
    expect(() => parseMapLayer('{"kind":"map_layer","featureCollection":{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[1,100]}}]}}')).toThrow("invalid geometry");
    expect(() => parseMapLayer('{"kind":"custom_table"}')).toThrow();
  });
  test("render IDs and layer colors do not overwrite source properties", () => {
    const value = layer();
    const rendered = renderCollection([value]);
    expect(rendered.features[0].id).toBe("layer:0");
    expect(rendered.features[0].properties?._cvLayer).toBe("layer");
    expect(value.collection.features[0].id).toBe("original-id");
    expect(value.collection.features[0].properties).not.toHaveProperty("_cvLayer");
    expect(featureName(value.collection.features[0], 0)).toBe("<b>Literal name</b>");
  });
  test("fits date-line tracks in the shortest longitude interval", () => {
    expect(geometryBounds([{ type: "LineString", coordinates: [[179, 10], [-179, 20]] }])).toEqual([[179, 10], [181, 20]]);
    expect(geometryBounds([])).toBeNull();
    const geometry: GeoJSON.LineString = { type: "LineString", coordinates: [[179,10],[-179,20]] };
    expect(renderGeometry(geometry)).toEqual({ type: "LineString", coordinates: [[179,10],[181,20]] });
    expect(geometry.coordinates[1][0]).toBe(-179);
  });
  test("bounds include polygons, holes and nested geometry collections", () => {
    expect(geometryBounds([{ type: "GeometryCollection", geometries: [
      { type: "Point", coordinates: [-73, 40] },
      { type: "Polygon", coordinates: [[[-74, 41], [-72, 41], [-72, 42], [-74, 41]]] },
    ] }])).toEqual([[-74, 40], [-72, 42]]);
  });
});
