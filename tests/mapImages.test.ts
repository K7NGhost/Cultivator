import { describe, expect, test } from "bun:test";
import { hasBasemapStyle, installBasemapImageResolver, missingBasemapImage } from "../src/lib/maps/mapImages";

describe("basemap image and loading recovery", () => {
  test("supplies visible circle and wood images without network requests", () => {
    for (const id of ["circle-11", "wood-pattern"]) {
      const image = missingBasemapImage(id)!;
      expect(image.data.length).toBe(image.width * image.height * 4);
      const alpha = [...image.data].filter((_, index) => index % 4 === 3);
      expect(alpha.some((value) => value > 0)).toBe(true);
      expect(new Set(alpha).size).toBeGreaterThan(1);
    }
    expect(missingBasemapImage("circle-11")?.sdf).toBe(true);
    expect(missingBasemapImage("wood-pattern")?.sdf).toBe(false);
    expect(missingBasemapImage("unknown-custom-icon")).toBeNull();
  });

  test("resolves pending MapLibre 6 requests, leaves existing icons intact and survives style reloads", async () => {
    const images = new Set<string>();
    const added: string[] = [];
    let resolver: ((id: string) => void | Promise<void>) | null = null;
    const map = {
      hasImage: (id: string) => images.has(id),
      addImage: (id: string) => { images.add(id); added.push(id); return map; },
      setMissingStyleImageResolver: (callback: typeof resolver) => { resolver = callback; return map; },
    };
    installBasemapImageResolver(map as unknown as Parameters<typeof installBasemapImageResolver>[0]);
    const resolve = resolver as unknown as (id: string) => void | Promise<void>;
    await resolve("circle-11");
    expect(images.has("circle-11")).toBe(true);
    await resolve("circle-11");
    expect(added).toEqual(["circle-11"]);
    images.clear();
    await resolve("circle-11");
    await resolve("wood-pattern");
    await resolve("unknown-custom-icon");
    expect(added).toEqual(["circle-11", "circle-11", "wood-pattern"]);
    expect(images.has("unknown-custom-icon")).toBe(false);
  });

  test("a parsed basemap is retained when an optional resource fails before overlays are added", () => {
    const map = (style: unknown) => ({ getStyle: () => style }) as Parameters<typeof hasBasemapStyle>[0];
    expect(hasBasemapStyle(map({ layers: [{ id: "background", type: "background" }] }))).toBe(true);
    expect(hasBasemapStyle(map(undefined))).toBe(false);
    expect(hasBasemapStyle(map({ layers: [] }))).toBe(false);
  });
});
