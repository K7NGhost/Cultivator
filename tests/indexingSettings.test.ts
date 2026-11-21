import { describe, expect, test } from "bun:test";
import { defaultIndexingOptions, loadIndexingOptions } from "../src/features/settings/indexingSettings";

describe("indexing preferences", () => {
  test("restores each behavior and rejects corrupt or unbounded settings", () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
    let saved: string | null = null;
    Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: () => saved } });
    try {
      expect(loadIndexingOptions()).toEqual(defaultIndexingOptions);
      for (const mode of ["adaptive", "background", "manual"]) {
        saved = JSON.stringify({ mode, scanSeconds: 120, maxFileMib: 4 });
        expect(loadIndexingOptions()).toEqual({ mode, scanSeconds: 120, maxFileMib: 4 });
      }
      for (const invalid of ["{", "null", JSON.stringify({ mode: "unknown", scanSeconds: 60, maxFileMib: 8 }), JSON.stringify({ mode: "manual", scanSeconds: 0, maxFileMib: 8 }), JSON.stringify({ mode: "adaptive", scanSeconds: 60, maxFileMib: 1024 })]) {
        saved = invalid;
        expect(loadIndexingOptions()).toEqual(defaultIndexingOptions);
      }
    } finally {
      if (original) Object.defineProperty(globalThis, "localStorage", original);
      else Reflect.deleteProperty(globalThis, "localStorage");
    }
  });
});
