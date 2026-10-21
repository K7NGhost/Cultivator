type Layout = { columns: number; rowHeight: number };

// Preserve the first visible item when a tile-size or window-width change
// changes the number of columns. Clamp after filtering/replacing a collection.
export function galleryScrollAnchor(scrollTop: number, previous: Layout, next: Layout, total: number, height: number) {
  const first = Math.floor(scrollTop / previous.rowHeight) * previous.columns;
  const top = Math.floor(first / next.columns) * next.rowHeight;
  const maximum = Math.max(0, Math.ceil(total / next.columns) * next.rowHeight - height);
  return Math.max(0, Math.min(top, maximum));
}
