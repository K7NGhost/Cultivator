import type { Map as LibreMap } from "maplibre-gl";

type StyleImage = { width: number; height: number; data: Uint8Array; sdf: boolean };

/** Local replacements for the two absent OpenFreeMap dark-style sprites. */
export function missingBasemapImage(id: string): StyleImage | null {
  if (id === "circle-11") {
    const size = 11;
    const data = new Uint8Array(size * size * 4);
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const offset = (y * size + x) * 4;
        const distance = 3 - Math.hypot(x + 0.5 - size / 2, y + 0.5 - size / 2);
        data.set([255, 255, 255, Math.max(0, Math.min(255, Math.round(192 + distance * 32)))], offset);
      }
    }
    // A distance field lets the style's own icon-color work in either theme.
    return { width: size, height: size, data, sdf: true };
  }
  if (id === "wood-pattern") {
    const size = 16;
    const data = new Uint8Array(size * size * 4);
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const offset = (y * size + x) * 4;
        data.set([255, 255, 255, (x + y) % 8 === 0 ? 28 : 0], offset);
      }
    }
    return { width: size, height: size, data, sdf: false };
  }
  return null;
}

export function installBasemapImageResolver(map: Pick<LibreMap, "hasImage" | "addImage" | "setMissingStyleImageResolver">) {
  // In MapLibre 6, styleimagemissing is notification-only. This resolver runs
  // before the pending render request is completed, including after setStyle.
  map.setMissingStyleImageResolver((id) => {
    if (map.hasImage(id)) return;
    const image = missingBasemapImage(id);
    if (image) map.addImage(id, { width: image.width, height: image.height, data: image.data }, { sdf: image.sdf });
  });
}

/** Missing glyphs, sprites or individual tiles do not invalidate a parsed style. */
export function hasBasemapStyle(map: Pick<LibreMap, "getStyle">) {
  return Boolean(map.getStyle()?.layers?.length);
}
