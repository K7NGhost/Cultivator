type Job = { priority: () => number; start(): void; cancel(): void };

// Bounds the entire request, including disk reads and WebView image decoding.
export class ThumbnailWorkQueue {
  private jobs: Job[] = [];
  private running = 0;
  constructor(private concurrency: number) {}

  setConcurrency(value: number) { this.concurrency = Math.max(1, value); this.drain(); }

  enqueue<T>(work: () => Promise<T>, priority: () => number, signal: AbortSignal): Promise<T | null> {
    if (signal.aborted) return Promise.resolve(null);
    return new Promise((resolve, reject) => {
      const job: Job = {
        priority,
        cancel: () => {
          const index = this.jobs.indexOf(job);
          if (index >= 0) this.jobs.splice(index, 1);
          resolve(null);
        },
        start: () => {
          signal.removeEventListener("abort", job.cancel);
          this.running++;
          Promise.resolve().then(() => signal.aborted ? null : work()).then(resolve, reject).finally(() => {
            this.running--;
            this.drain();
          });
        },
      };
      signal.addEventListener("abort", job.cancel, { once: true });
      this.jobs.push(job);
      this.drain();
    });
  }

  private drain() {
    while (this.running < this.concurrency && this.jobs.length) {
      this.jobs.sort((a, b) => a.priority() - b.priority());
      this.jobs.shift()!.start();
    }
  }
}
