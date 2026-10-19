import type { MediaItem } from "../model/types";
import type { ThumbnailPriority } from "./mediaRepository";

export type PrefetchRange = { start: number; stop: number; visibleStart: number; visibleStop: number };
export function galleryPrefetchRange(start: number, stop: number, total: number, ahead: number, behind: number, direction: number, capacity: number): PrefetchRange {
  const screen = Math.max(1, stop - start + 1);
  // Keep the lookahead working set below half the decoded cache capacity, so
  // visited thumbnails can survive direction changes instead of being churned.
  const spare = Math.max(0, Math.floor(capacity / 2) - screen);
  const before = Math.min(screen * (direction < 0 ? ahead : behind), Math.floor(spare * (direction < 0 ? ahead : behind) / (ahead + behind)));
  const after = Math.min(screen * (direction < 0 ? behind : ahead), spare - before);
  return { start: Math.max(0, start - before), stop: Math.min(total - 1, stop + after), visibleStart: start, visibleStop: stop };
}

type Handle = { promise: Promise<string | null>; release(): void; setPriority(priority: ThumbnailPriority): void };
// Range-based requests are independent of mounted DOM rows. Only a bounded
// number are outstanding, and a jump cancels work outside the new lookahead.
export class GalleryPrefetch {
  private active = new Map<number, Handle>();
  private completed = new Set<number>();
  private range: PrefetchRange = { start: 0, stop: -1, visibleStart: 0, visibleStop: -1 };
  private disposed = false;
  constructor(private itemAt: (index: number) => MediaItem | undefined,
    private prepare: (item: MediaItem, priority: ThumbnailPriority) => Handle,
    private concurrency = 16) {}
  update(range: PrefetchRange) {
    this.disposed = false;
    this.range = range;
    for (const [index, request] of this.active) {
      if (index < range.start || index > range.stop) { request.release(); this.active.delete(index); }
      else request.setPriority(this.priority(index));
    }
    for (const index of this.completed) if (index < range.start || index > range.stop) this.completed.delete(index);
    this.drain();
  }
  dispose() { this.disposed = true; this.active.forEach((request) => request.release()); this.active.clear(); this.completed.clear(); }
  private priority(index: number): ThumbnailPriority { return index >= this.range.visibleStart && index <= this.range.visibleStop ? "visible" : "nearby"; }
  private drain() {
    if (this.disposed) return;
    const { start, stop, visibleStart, visibleStop } = this.range;
    const candidates: number[] = [];
    for (let i = start; i <= stop; i++) if (!this.completed.has(i) && !this.active.has(i)) candidates.push(i);
    candidates.sort((a, b) => Math.max(visibleStart - a, a - visibleStop, 0) - Math.max(visibleStart - b, b - visibleStop, 0));
    for (const index of candidates) {
      if (this.active.size >= this.concurrency) break;
      const item = this.itemAt(index);
      if (!item) continue;
      const request = this.prepare(item, this.priority(index));
      this.active.set(index, request);
      void request.promise.catch(() => null).finally(() => {
        request.release();
        if (this.active.get(index) !== request) return;
        this.active.delete(index);
        this.completed.add(index);
        this.drain();
      });
    }
  }
}
