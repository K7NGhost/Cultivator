import { describe, expect, test } from "bun:test";
import { formatTimestamp, parseTimestamp, timestampDisplayValue } from "../src/lib/timeZone";
import { formatArtifactTableCell } from "../src/features/artifacts/utils/artifactTableFormatting";
import { getTimeZone, saveTimeZone, availableTimeZones } from "../src/features/settings/timeZoneSettings";

describe("timezone settings and display", () => {
  test("converts UTC and explicit offsets with DST and retains original values", () => {
    expect(formatTimestamp("2026-01-11T12:00:00Z","America/New_York")).toBe("2026-01-11 07:00:00 America/New_York (UTC-05:00)");
    expect(formatTimestamp("2026-07-11T12:00:00Z","America/New_York")).toBe("2026-07-11 08:00:00 America/New_York (UTC-04:00)");
    expect(formatTimestamp("2026-07-11T14:00:00+02:00","Asia/Kolkata")).toBe("2026-07-11 17:30:00 Asia/Kolkata (UTC+05:30)");
    const value={time:"2026-01-11T12:00:00Z",coordinateTimes:[["2026-07-11T12:00:00Z",null]],date:"2026-07-11",unknown:"2026-07-11T12:00:00"};
    const original=JSON.stringify(value);
    expect(timestampDisplayValue(value,"America/New_York")).toMatchObject({time:"2026-01-11 07:00:00 America/New_York (UTC-05:00)",date:value.date,unknown:value.unknown});
    expect(JSON.stringify(value)).toBe(original);
    for(const invalid of ["2026-02-30T12:00:00Z","2026-11-11T24:00:00Z","2026-11-11T12:60:00Z","2026-11-11T12:00:60Z"]) expect(parseTimestamp(invalid)).toBeNull();
  });
  test("distinguishes repeated local clocks by their UTC offsets", () => {
    expect(formatTimestamp("2026-11-01T05:30:00Z","America/New_York")).toEndWith("(UTC-04:00)");
    expect(formatTimestamp("2026-11-01T06:30:00Z","America/New_York")).toEndWith("(UTC-05:00)");
  });
  test("formats typed artifact timestamps in the chosen zone while preserving IDs and raw clocks", () => {
    const column={key:"time",label:"Time",format:"datetime" as const,timestampUnit:"seconds" as const};
    expect(formatArtifactTableCell(1626796652,column,"America/New_York")).toBe("2021-07-20 11:57:32 America/New_York (UTC-04:00)");
    expect(formatArtifactTableCell("2026-11-11T01:00:00Z",{...column,format:"date"},"America/New_York")).toBe("2026-11-10");
    expect(formatArtifactTableCell("2026-11-11T01:00:00",column,"America/New_York")).toBe("2026-11-11T01:00:00");
    expect(formatArtifactTableCell(123,{key:"id",label:"ID"},"America/New_York")).toBe("123");
  });
  test("persists and restores a valid timezone and rejects invalid preferences", () => {
    const original=Object.getOwnPropertyDescriptor(globalThis,"localStorage");
    const values=new Map<string,string>();
    Object.defineProperty(globalThis,"localStorage",{configurable:true,value:{getItem:(key:string)=>values.get(key)??null,setItem:(key:string,value:string)=>values.set(key,value)}});
    try {
      expect(getTimeZone()).toBe("UTC");
      saveTimeZone("America/New_York"); expect(getTimeZone()).toBe("America/New_York");
      expect(availableTimeZones()).toContain("America/New_York");
      expect(()=>saveTimeZone("Invalid/Zone")).toThrow("valid timezone");
      expect(getTimeZone()).toBe("America/New_York");
      saveTimeZone("system");expect(getTimeZone()).toBe(Intl.DateTimeFormat().resolvedOptions().timeZone);
      values.set("cultivator.timeZone.v1","Invalid/Zone");expect(getTimeZone()).toBe("UTC");
    } finally {
      if(original)Object.defineProperty(globalThis,"localStorage",original);else Reflect.deleteProperty(globalThis,"localStorage");
    }
  });
});
