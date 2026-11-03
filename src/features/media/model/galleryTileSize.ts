export const GALLERY_TILE_SIZES = [
  { label: "Small", value: 112 },
  { label: "Medium", value: 144 },
  { label: "Large", value: 208 },
  { label: "Extra large", value: 288 },
  { label: "Jumbo", value: "two-columns" },
  { label: "Maximum", value: "one-column" },
] as const;

export type GalleryTileSize = typeof GALLERY_TILE_SIZES[number]["value"];

export function parseGalleryTileSize(value: string | null): GalleryTileSize {
  return GALLERY_TILE_SIZES.find((size) => String(size.value) === value)?.value ?? 144;
}

const storageKey = "cultivator.media.tileWidth";
export function readGalleryTileSize(): GalleryTileSize {
  try {
    return parseGalleryTileSize(localStorage.getItem(storageKey));
  } catch { return 144; }
}
export function saveGalleryTileSize(size: GalleryTileSize) {
  try { localStorage.setItem(storageKey, String(size)); } catch { /* Keep the session preference if storage is unavailable. */ }
}

export function galleryTileLayout(availableWidth: number, size: GalleryTileSize) {
  const width = Math.max(1, availableWidth);
  const columnCount = size === "two-columns" ? 2 : size === "one-column" ? 1
    : Math.max(1, Math.floor(width / size));
  return { columnCount, columnWidth: width / columnCount };
}
