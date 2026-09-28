import { describe, expect, test } from "bun:test";

import { parseGpsCoordinate, topologyPath, validPoint } from "../src/features/map/geo";

describe("parseGpsCoordinate", () => {
  test("reads EXIF degrees, minutes and seconds with the hemisphere", () => {
    expect(parseGpsCoordinate("37 deg 48 min 28.8 sec N")).toBeCloseTo(37.808, 3);
    expect(parseGpsCoordinate("122 deg 25 min 3.72 sec W")).toBeCloseTo(-122.4177, 3);
  });

  test("reads rationals and plain decimals", () => {
    expect(parseGpsCoordinate("122/1 25/1 372/100 W")).toBeCloseTo(-122.4177, 3);
    expect(parseGpsCoordinate("-33.8688")).toBeCloseTo(-33.8688, 4);
    expect(parseGpsCoordinate("unknown")).toBeNull();
  });

  test("rejects out-of-range and 0,0 points", () => {
    expect(validPoint(91, 10)).toBeNull();
    expect(validPoint(0, 0)).toBeNull();
    expect(validPoint(25.79, -80.13)).toEqual({ lat: 25.79, lon: -80.13 });
  });
});

describe("topologyPath", () => {
  test("decodes delta-encoded arcs, including reversed ones", () => {
    const topology = {
      transform: { scale: [1, 1] as [number, number], translate: [0, 0] as [number, number] },
      // Arc 0 runs (0,0) -> (10,0) -> (10,10); arc 1 runs (0,0) -> (0,10) -> (10,10).
      arcs: [
        [[0, 0], [10, 0], [0, 10]],
        [[0, 0], [0, 10], [10, 0]],
      ] as Array<Array<[number, number]>>,
      objects: { land: { geometries: [{ type: "Polygon", arcs: [[0, ~1]] }] } },
    };

    expect(topologyPath(topology, "land")).toBe(
      "M180.000,90.000L190.000,90.000L190.000,80.000L180.000,80.000L180.000,90.000Z",
    );
  });
});
