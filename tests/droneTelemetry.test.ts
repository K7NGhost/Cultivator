import { describe, expect, test } from "bun:test";
import {
  flightGeoJson,
  hasGps,
  mapCoordinates,
  selectedGeoJson,
} from "../src/features/drone/lib/mapData";
import {
  elapsedLabel,
  finiteLabel,
  pointAtTime,
} from "../src/features/drone/lib/telemetry";
import type { FlightRecord } from "../src/features/drone/types";

function point(
  id: number,
  elapsedSeconds: number,
  longitude: number | null = 0,
): FlightRecord {
  return {
    id,
    time: "",
    elapsedSeconds,
    latitude: 40,
    longitude,
    altitude: null,
    speed: null,
    battery: null,
    satellites: null,
    mode: null,
  };
}
describe("drone map and playback", () => {
  test("invalid and missing GPS never become plotted positions", () => {
    for (const row of [
      point(1, 0, null),
      point(1, 0, NaN),
      point(1, 0, 190),
      { ...point(1, 0), latitude: 0 },
    ]) {
      expect(hasGps(row)).toBe(false);
      expect(selectedGeoJson(row).features).toHaveLength(0);
    }
    expect(flightGeoJson([]).route.features).toHaveLength(0);
  });
  test("crossing the date line does not draw a route across the world", () => {
    const points = [point(1, 0, 179.9), point(2, 1, -179.9)].filter(hasGps);
    const coordinates = mapCoordinates(points);
    expect(coordinates[1][0] - coordinates[0][0]).toBeCloseTo(0.2);
    expect(
      selectedGeoJson(points[1], coordinates[0][0]).features[0].geometry
        .coordinates[0],
    ).toBeCloseTo(180.1);
  });
  test("playback follows recorded elapsed time rather than sample index", () => {
    const rows = [point(1, 0), point(2, 0.1), point(3, 15), point(4, 120)];
    expect(pointAtTime(rows, 10)).toBe(1);
    expect(pointAtTime(rows, 15)).toBe(2);
    expect(pointAtTime(rows, 999)).toBe(3);
    expect(pointAtTime(rows, -1)).toBe(0);
  });
  test("unknown telemetry stays unavailable", () => {
    expect(finiteLabel(null)).toBe("—");
    expect(finiteLabel(NaN)).toBe("—");
    expect(finiteLabel(0)).toBe("0");
    expect(elapsedLabel(3661)).toBe("01:01:01");
  });
});
