import type { MediaItem } from "../model/types";
import type { FileViewMediaPage } from "./mediaRepository";

// Keep database pages behind a continuous grid without retaining every visited item.
export class VirtualMediaCache {
  private pages = new Map<number, FileViewMediaPage>();
  private pending = new Set<number>();
  private wanted = new Set<number>();
  private errors = new Set<number>();
  private disposed = false;
  private byteBudget = 16 * 1024 * 1024;
  private pageBytes = new Map<number, number>();

  constructor(
    readonly firstPage: FileViewMediaPage,
    private fetchPage: (offset: number) => Promise<FileViewMediaPage>,
    private changed: () => void,
    private maxPages = 8,
  ) {
    this.pages.set(firstPage.offset, firstPage);
    this.pageBytes.set(firstPage.offset, this.estimate(firstPage));
  }

  get totalCount() { return this.firstPage.totalCount; }
  get hasError() { return this.errors.size > 0; }
  get cachedPageCount() { return this.pages.size; }
  setBudget(bytes: number) { this.byteBudget = bytes; this.maxPages = Number.MAX_SAFE_INTEGER; this.prune(); }

  itemAt(index: number): MediaItem | undefined {
    const offset = this.pageOffset(index);
    return this.pages.get(offset)?.items[index - offset];
  }

  setRange(start: number, stop: number) {
    const { limit, totalCount } = this.firstPage;
    const first = this.pageOffset(Math.max(0, start));
    const last = this.pageOffset(Math.min(stop, totalCount - 1));
    const wanted = new Set<number>();
    for (let offset = first; offset <= last; offset += limit) wanted.add(offset);
    // Prioritize the viewport, then prefetch adjacent database batches.
    if (last + limit < totalCount) wanted.add(last + limit);
    if (first > 0) wanted.add(first - limit);
    this.wanted = wanted;
    for (const offset of this.errors) {
      if (!wanted.has(offset)) this.errors.delete(offset);
    }
    for (const offset of wanted) {
      const page = this.pages.get(offset);
      if (page) { this.pages.delete(offset); this.pages.set(offset, page); }
    }
    this.prune();
    this.drain();
  }

  retry() {
    this.errors.clear();
    this.changed();
    this.drain();
  }

  activate() { this.disposed = false; this.drain(); }
  dispose() { this.disposed = true; }

  private pageOffset(index: number) {
    return Math.floor(index / this.firstPage.limit) * this.firstPage.limit;
  }

  private prune() {
    let bytes = [...this.pageBytes.values()].reduce((sum, size) => sum + size, 0);
    for (const offset of this.pages.keys()) {
      if (this.pages.size <= Math.max(this.maxPages, this.wanted.size) && bytes <= this.byteBudget) break;
      if (!this.wanted.has(offset)) {
        bytes -= this.pageBytes.get(offset) ?? 0;
        this.pages.delete(offset);
        this.pageBytes.delete(offset);
      }
    }
  }

  private drain() {
    if (this.disposed) return;
    for (const offset of this.wanted) {
      if (this.pending.size >= 2) break;
      if (this.pages.has(offset) || this.pending.has(offset) || this.errors.has(offset)) continue;
      this.pending.add(offset);
      this.fetchPage(offset).then((page) => {
        if (!this.disposed && this.wanted.has(offset)) {
          this.pages.set(offset, page);
          this.pageBytes.set(offset, this.estimate(page));
          this.prune();
          this.changed();
        }
      }).catch(() => {
        if (!this.disposed && this.wanted.has(offset)) {
          this.errors.add(offset);
          this.changed();
        }
      }).finally(() => { this.pending.delete(offset); this.drain(); });
    }
  }

  private estimate(page: FileViewMediaPage) { return JSON.stringify(page).length * 2 + page.items.length * 256; }
}
