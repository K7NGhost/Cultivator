export const GALLERY_TILE_SIZES = [
  { label: "Small", width: 112 },
  { label: "Medium", width: 144 },
  { label: "Large", width: 208 },
  { label: "Extra large", width: 288 },
] as const;

const storageKey = "cultivator.media.tileWidth";
export function readGalleryTileWidth() {
  try {
    const stored = Number(localStorage.getItem(storageKey));
    return GALLERY_TILE_SIZES.find((size) => size.width === stored)?.width ?? 144;
  } catch { return 144; }
}
export function saveGalleryTileWidth(width: number) {
  try { localStorage.setItem(storageKey, String(width)); } catch { /* Keep the session preference if storage is unavailable. */ }
}
