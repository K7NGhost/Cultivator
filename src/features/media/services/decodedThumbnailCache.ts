export type DecodedThumbnail = { source: string; bytes: number; dispose: () => void };

// Strong image references retain browser resources without retaining React tiles.
// Chromium still controls its decoded-image working set; this is an estimated
// application cache budget, not an OS process memory limit.
export class DecodedThumbnailCache {
  private entries = new Map<string, DecodedThumbnail>();
  private listeners = new Map<string, Set<() => void>>();
  private bytes = 0;
  private retained = new Map<string, number>();
  constructor(private budget: number) {}
  get size() { return this.entries.size; }
  get usedBytes() { return this.bytes; }
  retain(key: string) {
    this.retained.set(key, (this.retained.get(key) ?? 0) + 1);
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const count = (this.retained.get(key) ?? 1) - 1;
      if (count > 0) this.retained.set(key, count); else this.retained.delete(key);
      this.prune();
    };
  }
  peek(key: string) { return this.entries.get(key)?.source; }
  subscribe(key: string, listener: () => void) {
    let subscribers = this.listeners.get(key);
    if (!subscribers) { subscribers = new Set(); this.listeners.set(key, subscribers); }
    subscribers.add(listener);
    return () => {
      subscribers.delete(listener);
      if (!subscribers.size) this.listeners.delete(key);
    };
  }
  get(key: string) {
    const value = this.entries.get(key);
    if (value) { this.entries.delete(key); this.entries.set(key, value); }
    return value?.source;
  }
  put(key: string, value: DecodedThumbnail) {
    this.remove(key, false);
    if (value.bytes > this.budget) { value.dispose(); this.changed(key); return; }
    this.entries.set(key, value);
    this.bytes += value.bytes;
    this.prune();
    this.changed(key);
  }
  setBudget(bytes: number) { this.budget = Math.max(0, bytes); this.prune(); }
  clear() { for (const key of this.entries.keys()) this.remove(key); }
  private prune() {
    for (const key of this.entries.keys()) {
      if (this.bytes <= this.budget) break;
      if (this.retained.has(key)) continue;
      this.remove(key);
    }
    // Retention favors the inspector preview, but never overrides the budget.
    for (const key of this.entries.keys()) {
      if (this.bytes <= this.budget) break;
      this.remove(key);
    }
  }
  private changed(key: string) { this.listeners.get(key)?.forEach((listener) => listener()); }
  private remove(key: string, notify = true) {
    const value = this.entries.get(key);
    if (value) {
      this.bytes -= value.bytes; this.entries.delete(key); value.dispose();
      if (notify) this.changed(key);
    }
  }
}

export async function decodeThumbnail(path: string, signal?: AbortSignal): Promise<DecodedThumbnail> {
  const response = await fetch(path, { signal, cache: "no-store" });
  if (!response.ok) throw new Error("Thumbnail could not be read.");
  const blob = await response.blob();
  signal?.throwIfAborted();
  if (blob.size > 2 * 1024 * 1024) throw new Error("Thumbnail exceeds the preview size limit.");
  // Revocable URLs let eviction release browser resources as well as JS entries.
  const source = URL.createObjectURL(blob);
  const image = new Image();
  image.decoding = "async";
  const dispose = () => { image.removeAttribute("src"); URL.revokeObjectURL(source); };
  const abort = () => image.removeAttribute("src");
  signal?.addEventListener("abort", abort, { once: true });
  image.src = source;
  try {
    await image.decode();
    signal?.throwIfAborted();
    if (image.naturalWidth > 256 || image.naturalHeight > 256) throw new Error("Invalid thumbnail dimensions.");
  } catch (error) { dispose(); throw error; }
  finally { signal?.removeEventListener("abort", abort); }
  return { source, bytes: image.naturalWidth * image.naturalHeight * 4 + blob.size + 4096, dispose };
}
