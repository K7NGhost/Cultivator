import { describe, expect, test } from "bun:test";
import { featureTimeRange, filterMapDates } from "../src/features/mapping/lib/dateFilter";
import { featureAtSourceIndex, renderCollection } from "../src/features/mapping/lib/mapFeatures";
import type { MapFeature, VisibleMapLayer } from "../src/features/mapping/types";

function point(properties: Record<string, unknown> = {}): MapFeature {
  return { type: "Feature", properties, geometry: { type: "Point", coordinates: [-73, 40] } };
}
function layer(features: MapFeature[]): VisibleMapLayer {
  return { id: "source", label: "Dates.kml", datasourceId: "datasource", filePath: "/Dates.kml", format: "kml",
    featureCount: features.length, positionCount: features.length, color: "#f97316", warnings: [],
    collection: { type: "FeatureCollection", features } };
}
const oneDay = { from: "2026-11-11", to: "2026-11-11", includeUndated: false };

describe("map date range", () => {
  test("uses the selected timezone for calendar days and daily time windows", () => {
    const value = layer([point({time:"2026-11-12T03:00:00Z"}), point({time:"2026-11-11T15:00:00Z"}), point({time:"2026-11-11T04:59:59Z"})]);
    expect(filterMapDates([value], oneDay, "America/New_York").layers[0].featureIndices).toEqual([0,1]);
    expect(filterMapDates([value], {...oneDay,fromTime:"21:00",toTime:"23:00"}, "America/New_York").layers[0].featureIndices).toEqual([0]);
    expect(filterMapDates([value], {...oneDay,fromTime:"09:00",toTime:"10:00"}, "America/New_York").layers[0].featureIndices).toEqual([1]);
  });
  test("supports time-only, open-ended, and overnight daily ranges", () => {
    const value = layer([point({time:"2026-11-11T23:00:00Z"}),point({time:"2026-11-12T01:00:00Z"}),point({time:"2026-11-12T12:00:00Z"})]);
    const filter = {from:"",to:"",fromTime:"22:00",toTime:"02:00",includeUndated:false};
    expect(filterMapDates([value], filter).layers[0].featureIndices).toEqual([0,1]);
    expect(filterMapDates([value], {...filter,toTime:""}).layers[0].featureIndices).toEqual([0]);
    expect(filterMapDates([value], {...filter,fromTime:""}).layers[0].featureIndices).toEqual([1]);
    expect(filterMapDates([value], {...filter,...oneDay}).layers[0].featureIndices).toEqual([0]);
  });
  test("includes the complete entered ending minute or second", () => {
    const value=layer([point({time:"2026-11-11T12:30:59.999Z"}),point({time:"2026-11-11T12:31:00Z"}),point({time:"2026-11-11T12:30:20.999Z"})]);
    expect(filterMapDates([value],{...oneDay,fromTime:"12:30",toTime:"12:30"}).layers[0].featureIndices).toEqual([0,2]);
    expect(filterMapDates([value],{...oneDay,fromTime:"12:30:20",toTime:"12:30:20"}).layers[0].featureIndices).toEqual([2]);
    expect(filterMapDates([value],{...oneDay,fromTime:"25:00"}).error).toContain("valid start and end times");
  });
  test("does not invent a timezone or a time of day for unspecified values", () => {
    const value=layer([point({time:"2026-11-11T10:00:00"}),point({time:"2026-11-11"}),point({time:"2026-11-12"}),point()]);
    const filter={...oneDay,fromTime:"09:00",toTime:"11:00"};
    expect(filterMapDates([value],filter,"Asia/Tokyo").layers[0].featureIndices).toEqual([0]);
    expect(filterMapDates([value],{...filter,includeUndated:true},"Asia/Tokyo").layers[0].featureIndices).toEqual([0,1,3]);
  });
  test("retains whole tracks that overlap a daily window even when endpoints are outside", () => {
    const track={...point({startTime:"2026-11-11T08:00:00Z",endTime:"2026-11-11T18:00:00Z"}),geometry:{type:"LineString" as const,coordinates:[[1,2],[3,4]]}};
    const view=filterMapDates([layer([track])],{...oneDay,fromTime:"12:00",toTime:"13:00"});
    expect(view.matched).toBe(1);
    expect(view.layers[0].collection.features[0]).toBe(track);
  });
  test("matches repeated DST hours and avoids nonexistent spring clocks", () => {
    const zone="America/New_York";
    const repeated=layer([point({time:"2026-11-01T05:30:00Z"}),point({time:"2026-11-01T06:30:00Z"})]);
    expect(filterMapDates([repeated],{from:"2026-11-01",to:"2026-11-01",fromTime:"01:30",toTime:"01:30",includeUndated:false},zone).matched).toBe(2);
    const reversedClock=layer([point({startTime:"2026-11-01T05:50:00Z",endTime:"2026-11-01T06:10:00Z"})]);
    for(const time of ["01:55","01:05"]) expect(filterMapDates([reversedClock],{from:"",to:"",fromTime:time,toTime:time,includeUndated:false},zone).matched).toBe(1);
    const spring=layer([point({startTime:"2026-03-08T06:30:00Z",endTime:"2026-03-08T07:30:00Z"})]);
    expect(filterMapDates([spring],{from:"2026-03-08",to:"2026-03-08",fromTime:"02:00",toTime:"02:59",includeUndated:false},zone).matched).toBe(0);
  });
  test("handles fractional offsets and long track ranges without scanning every day", () => {
    const value=layer([point({startTime:"2000-01-01T00:00:00Z",endTime:"2040-01-01T00:00:00Z"})]);
    expect(filterMapDates([value],{...oneDay,fromTime:"12:34:56",toTime:"12:34:56"},"Asia/Kolkata").matched).toBe(1);
    const points=layer([point({time:"2026-11-10T18:30:00Z"}),point({time:"2026-11-11T18:29:59Z"}),point({time:"2026-11-11T18:30:00Z"})]);
    expect(filterMapDates([points],oneDay,"Asia/Kolkata").layers[0].featureIndices).toEqual([0,1]);
  });
  test("includes both full boundary days and converts explicit offsets to UTC", () => {
    const value = layer([
      point({ time: "2026-11-11T00:00:00Z" }), point({ time: "2026-11-11T23:59:59.999Z" }),
      point({ time: "2026-11-10T23:00:00-02:00" }), point({ time: "2026-11-11T23:00:00-02:00" }),
      point({ time: "2026-11-12T00:00:00Z" }),
    ]);
    const view = filterMapDates([value], oneDay);
    expect(view.matched).toBe(3);
    expect(view.layers[0].featureIndices).toEqual([0,1,2]);
    expect(value.collection.features).toHaveLength(5);
  });

  test("omits undated features during filtering unless explicitly included", () => {
    const value = layer([point({ time: "2026-11-11" }), point(), point({ time: "not a date" })]);
    expect(filterMapDates([value], oneDay)).toMatchObject({ matched: 1, total: 3, undated: 2 });
    expect(filterMapDates([value], { ...oneDay, includeUndated: true }).matched).toBe(3);
    const cleared = filterMapDates([value], { from: "", to: "", includeUndated: false });
    expect(cleared.matched).toBe(3);
    expect(cleared.layers[0]).toBe(value);
  });

  test("supports start-only and end-only ranges", () => {
    const value = layer([point({ date: "2026-11-10" }), point({ date: "2026-11-11" }), point({ date: "2026-11-12" })]);
    expect(filterMapDates([value], { ...oneDay, to: "" }).layers[0].featureIndices).toEqual([1,2]);
    expect(filterMapDates([value], { ...oneDay, from: "" }).layers[0].featureIndices).toEqual([0,1]);
  });

  test("matches track and interval features when their recorded time range overlaps", () => {
    const value = layer([
      point({ coordinateTimes: [["2026-11-10T23:00:00Z", null], ["2026-11-12T01:00:00Z"]] }),
      point({ startTime: "2026-11-01", endTime: "2026-11-11" }),
      point({ coordinateTimes: [null, "2026-11-12T01:00:00Z"] }),
    ]);
    expect(filterMapDates([value], oneDay).layers[0].featureIndices).toEqual([0,1]);
  });

  test("keeps recorded dates when a timestamp has no timezone", () => {
    const value = layer([point({ time: "2026-11-11T00:00:00" }), point({ date: "2026-11-11" })]);
    expect(filterMapDates([value], oneDay).matched).toBe(2);
    expect(featureTimeRange(value.collection.features[0])?.start).toBe(Date.parse("2026-11-11T00:00:00Z"));
  });

  test("rejects invalid dates, reversed ranges and ambiguous timestamps", () => {
    const value = layer([point({ time: "2026-11-11" })]);
    expect(filterMapDates([value], { ...oneDay, from: "2026-11-12" }).error).toContain("start date");
    expect(filterMapDates([value], { ...oneDay, from: "2026-02-30" }).error).toContain("valid");
    for (const time of ["2026-02-30T10:00:00Z", "11/12/2026", 1794355200, "1794355200000"]) {
      expect(featureTimeRange(point({ time }))).toBeNull();
    }
  });

  test("recognizes named GeoJSON timestamps and KML ExtendedData", () => {
    expect(filterMapDates([layer([point({ recorded_at: "2026-11-11T12:00:00Z" }),
      point({ extendedData: { DateTime: "2026-11-11T13:00:00Z" } })])], oneDay).matched).toBe(2);
  });

  test("preserves feature identities for map clicks and selection after filtering", () => {
    const original = layer([point({ time: "2026-11-10", name: "Outside" }), point({ time: "2026-11-11", name: "Match" })]);
    const filtered = filterMapDates([original], oneDay).layers[0];
    expect(renderCollection([filtered]).features[0].id).toBe("source:1");
    expect(renderCollection([filtered]).features[0].properties?._cvIndex).toBe(1);
    expect(featureAtSourceIndex(filtered, 1)).toBe(original.collection.features[1]);
    expect(featureAtSourceIndex(filtered, 0)).toBeUndefined();
    expect(filtered).toMatchObject({ featureCount: 1, totalFeatureCount: 2 });
  });
});
