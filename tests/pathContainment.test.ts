import { describe, expect, test } from "bun:test";

import {
  isSameOrInside,
  sourcesContainingCaseFolder,
} from "../src/features/datasources/pathContainment";

describe("path containment", () => {
  test("matches the folder itself and anything inside it, ignoring case and slashes", () => {
    expect(isSameOrInside("C:\\Evidence\\demo-phone", "C:/evidence/demo-phone/")).toBe(true);
    expect(isSameOrInside("C:/Evidence/demo-phone/be", "C:\\Evidence\\demo-phone")).toBe(true);
  });

  test("does not match a sibling that only shares a name prefix", () => {
    expect(isSameOrInside("C:/Evidence/demo-phone-2", "C:/Evidence/demo-phone")).toBe(false);
  });

  test("finds the sources that would pull the case's own files in as evidence", () => {
    const caseFolder = "C:/Users/amire/OneDrive/Documents/Cultivator Demo/demo-phone/be";

    expect(
      sourcesContainingCaseFolder(
        [
          "C:\\Users\\amire\\OneDrive\\Documents\\Cultivator Demo\\demo-phone",
          "C:\\Users\\amire\\OneDrive\\Documents\\Cultivator Demo\\device-001",
        ],
        caseFolder,
      ),
    ).toEqual(["C:\\Users\\amire\\OneDrive\\Documents\\Cultivator Demo\\demo-phone"]);
    expect(sourcesContainingCaseFolder(["D:/phone"], "C:/Cases/case-1")).toEqual([]);
  });
});
