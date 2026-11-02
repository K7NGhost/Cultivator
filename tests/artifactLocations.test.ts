import { describe, expect, test } from "bun:test";
import { detectCoordinatePairs, getArtifactLocations, locationBounds } from "../src/features/artifacts/utils/artifactLocations";
import { getArtifactTableView } from "../src/features/artifacts/utils/artifactTableView";
import type { CustomTableView } from "../src/features/artifacts/utils/customTableGrouping";
import type { StoredArtifactRecord } from "../src/features/artifacts/model/types";

function table(columns: { key: string; label: string }[], values: Record<string, unknown>[]): CustomTableView {
  return { name: "Locations", columns, deduplication: { mode: "preserve" }, totalOccurrences: values.length,
    sources: [{ artifactId: "artifact", filePath: "evidence.db" }],
    rows: values.map((values) => ({ values, occurrenceCount: 1, primarySourceIndex: 0 })) };
}
const columns = [{ key: "latitude", label: "Latitude" }, { key: "longitude", label: "Longitude" }];

describe("artifact location preview data", () => {
  test.each([
    ["latitude", "longitude"], ["Lat", "Lng"], ["GPSLatitude", "GPSLongitude"],
    ["ZLATITUDE", "ZLONGITUDE"], ["latitude_degrees", "longitude_degrees"],
  ])("recognizes %s and %s", (latitude, longitude) => {
    expect(detectCoordinatePairs([{ key: latitude, label: latitude }, { key: longitude, label: longitude }])).toHaveLength(1);
  });

  test("detects friendly labels when keys are generic", () => {
    expect(detectCoordinatePairs([{ key: "col1", label: "Latitude (degrees)" }, { key: "col2", label: "Longitude (degrees)" }]))
      .toEqual([{ latitude: "col1", longitude: "col2", label: "Location" }]);
  });

  test("keeps origin and destination pairs separate and does not cross-match unrelated columns", () => {
    const cols = ["origin_latitude", "destination_longitude", "destination_latitude", "origin_longitude", "home_latitude"]
      .map((key) => ({ key, label: key }));
    expect(detectCoordinatePairs(cols)).toEqual([
      { latitude: "origin_latitude", longitude: "origin_longitude", label: "origin" },
      { latitude: "destination_latitude", longitude: "destination_longitude", label: "destination" },
    ]);
  });

  test("does not interpret coordinate deltas, scaled integers or radians as positions", () => {
    const cols = ["delta_latitude", "delta_longitude", "latitude_e7", "longitude_e7", "latitude_radians", "longitude_radians"]
      .map((key) => ({ key, label: key }));
    expect(detectCoordinatePairs(cols)).toEqual([]);
    expect(detectCoordinatePairs([{ key: "latitude_e7", label: "Latitude" }, { key: "longitude_e7", label: "Longitude" }])).toEqual([]);
    expect(detectCoordinatePairs([{ key: "latitude", label: "Latitude radians" }, { key: "longitude", label: "Longitude radians" }])).toEqual([]);
  });

  test("rejects invalid and missing coordinates without turning blanks into zero", () => {
    const input = table(columns, [
      { latitude: "40.25", longitude: " -74.5 " }, { latitude: 0, longitude: 10 },
      { latitude: "", longitude: 10 }, { latitude: null, longitude: 10 }, { latitude: false, longitude: 10 },
      { latitude: 91, longitude: 10 }, { latitude: 40, longitude: 181 }, { latitude: 0, longitude: 0 },
      { latitude: NaN, longitude: 0 }, { latitude: "40junk", longitude: 10 }, { latitude: 40, longitude: Infinity },
    ]);
    const before = JSON.stringify(input);
    const result = getArtifactLocations(input);
    expect(result.points).toEqual([
      { latitude: 40.25, longitude: -74.5, rowIndex: 0, pairLabel: "Location" },
      { latitude: 0, longitude: 10, rowIndex: 1, pairLabel: "Location" },
    ]);
    expect(result.invalidRows).toBe(9);
    expect(JSON.stringify(input)).toBe(before);
  });

  test("retains full row indices and advanced information for each pin", () => {
    const input = table(columns, [{ latitude: null, longitude: null },
      { latitude: 51, longitude: -0.1, address: "Example", timestamp: 1626796652 }]);
    const result = getArtifactLocations(input);
    expect(input.rows[result.points[0].rowIndex].values).toMatchObject({ address: "Example", timestamp: 1626796652 });
    expect(input.sources[input.rows[result.points[0].rowIndex].primarySourceIndex].filePath).toBe("evidence.db");
  });

  test("supports standardized models and custom tables through the same adapter", () => {
    const artifact: StoredArtifactRecord = { id: "one", resultKind: "location", label: "Place", filePath: "source.db",
      payload: { kind: "location", category: "locations", label: "Place", latitude: 51, longitude: -0.1 },
      createdAt: "", datasourceId: "source", pluginId: "plugin", jobId: "job" };
    const result = getArtifactLocations(getArtifactTableView([artifact])!);
    expect(result.points).toHaveLength(1);
    expect(result.points[0].latitude).toBe(51);
  });

  test("fits date-line positions using the short span and handles empty or single points", () => {
    const point = (latitude: number, longitude: number) => ({ latitude, longitude, rowIndex: 0, pairLabel: "Location" });
    expect(locationBounds([])).toBeNull();
    expect(locationBounds([point(40, -74)])).toEqual([[-74, 40], [-74, 40]]);
    expect(locationBounds([point(1, 179), point(2, -179)])).toEqual([[179, 1], [181, 2]]);
  });
});
