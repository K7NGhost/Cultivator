export type DecodedThumbnail = { source: string; bytes: number; dispose: () => void };

// Strong image references retain browser resources without retaining React tiles.
// Chromium still controls its decoded-image working set; this is an estimated
// application cache budget, not an OS process memory limit.
export class DecodedThumbnailCache {
  private entries = new Map<string, DecodedThumbnail>();
  private listeners = new Map<string, Set<() => void>>();
  private bytes = 0;
  constructor(private budget: number) {}
  get size() { return this.entries.size; }
  get usedBytes() { return this.bytes; }
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

export async function decodeThumbnail(source: string): Promise<DecodedThumbnail> {
  const image = new Image();
  image.decoding = "async";
  image.src = source;
  try { await image.decode(); }
  catch (error) { image.removeAttribute("src"); throw error; }
  return { source, bytes: image.naturalWidth * image.naturalHeight * 4 + 4096, dispose: () => image.removeAttribute("src") };
}
