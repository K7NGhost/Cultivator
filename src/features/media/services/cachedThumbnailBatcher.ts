import type { CachedThumbnailHit, CachedThumbnailRequest } from "./mediaRepository";

type Lookup = (casePath: string, requests: CachedThumbnailRequest[]) => Promise<CachedThumbnailHit[]>;
type Waiting = { casePath: string; id: string; sourcePath: string; signal: AbortSignal; priority(): number; resolve(value: string | null): void; reject(error: unknown): void; cancel(): void };

// Cache reads have their own batched path and never acquire a generation slot.
// Cancellation removes individual consumers without aborting unrelated tiles.
export class CachedThumbnailBatcher {
  private waiting: Waiting[] = [];
  private scheduled = false;
  private nextId = 0;
  private running = 0;
  constructor(private lookup: Lookup, private batchSize = 128, private concurrency = 2) {}

  read(casePath: string, sourcePath: string, signal: AbortSignal, priority = () => 0): Promise<string | null> {
    if (signal.aborted) return Promise.resolve(null);
    return new Promise((resolve, reject) => {
      const request: Waiting = {
        casePath, id: String(++this.nextId), sourcePath, signal, priority, resolve, reject,
        cancel: () => { resolve(null); this.schedule(); },
      };
      signal.addEventListener("abort", request.cancel, { once: true });
      this.waiting.push(request);
      this.schedule();
    });
  }

  private schedule() {
    if (!this.scheduled) { this.scheduled = true; queueMicrotask(() => this.flush()); }
  }

  private flush() {
    this.scheduled = false;
    this.waiting = this.waiting.filter(request => {
        if (!request.signal.aborted) return true;
        request.signal.removeEventListener("abort", request.cancel);
        return false;
      });
    this.waiting.sort((a, b) => a.priority() - b.priority());
    while (this.running < this.concurrency && this.waiting.length) {
        const casePath = this.waiting[0].casePath;
        const batch = this.waiting.filter(r => r.casePath === casePath).slice(0, this.batchSize);
        const selected = new Set(batch);
        this.waiting = this.waiting.filter(r => !selected.has(r));
        this.running++;
        void this.lookup(casePath, batch.map(({ id, sourcePath }) => ({ id, sourcePath }))).then(hits => {
          const paths = new Map(hits.map(hit => [hit.id, hit.path]));
          for (const request of batch) request.resolve(request.signal.aborted ? null : paths.get(request.id) ?? null);
        }, error => {
          for (const request of batch) if (!request.signal.aborted) request.reject(error);
        }).finally(() => {
          for (const request of batch) request.signal.removeEventListener("abort", request.cancel);
          this.running--;
          this.schedule();
        });
    }
  }
}
