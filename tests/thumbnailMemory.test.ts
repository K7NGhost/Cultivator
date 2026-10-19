import { describe, expect, test } from "bun:test";
import { ThumbnailWorkQueue } from "../src/features/media/services/thumbnailWorkQueue";
import { DecodedThumbnailCache } from "../src/features/media/services/decodedThumbnailCache";
import { galleryPrefetchRange } from "../src/features/media/services/galleryPrefetch";
import {
  automaticPerformanceSettings, normalizePerformanceSettings, performancePresets,
  nativeMemoryMb, thumbnailCacheBytes, recordCacheBytes, thumbnailConcurrency,
} from "../src/features/settings/performanceSettings";

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("bounded gallery memory", () => {
  test("unknown and low-memory hardware start conservatively; faster presets scale up", () => {
    expect(automaticPerformanceSettings()).toEqual(performancePresets["Memory saver"]);
    expect(automaticPerformanceSettings(2)).toEqual(performancePresets["Memory saver"]);
    expect(automaticPerformanceSettings(8)).toEqual(performancePresets.Balanced);
    expect(thumbnailConcurrency(performancePresets["Memory saver"])).toBe(2);
    expect(thumbnailConcurrency(performancePresets["High performance"])).toBe(8);
    expect(thumbnailCacheBytes(performancePresets["Memory saver"])).toBe(32 * 1048576);
    for (const preset of Object.values(performancePresets)) {
      expect(nativeMemoryMb(preset) * 1048576 + thumbnailCacheBytes(preset) + recordCacheBytes(preset))
        .toBeLessThan(preset.memoryMb * 1048576);
    }
  });

  test("minimal budgets and disabling lookahead remain valid", () => {
    expect(normalizePerformanceSettings({ memoryMb: 1, screensAhead: 0, screensBehind: 0 }))
      .toEqual({ memoryMb: 192, screensAhead: 0, screensBehind: 0 });
    expect(galleryPrefetchRange(100, 119, 1000, 0, 0, 1, 80))
      .toEqual({ start: 100, stop: 119, visibleStart: 100, visibleStop: 119 });
    expect(nativeMemoryMb(normalizePerformanceSettings({ memoryMb: 8192 }))).toBe(4096);
  });

  test("eviction, shrinking the budget, and clearing dispose every cached resource", () => {
    const disposed: string[] = [];
    const cache = new DecodedThumbnailCache(20);
    const put = (key: string) => cache.put(key, { source: key, bytes: 10, dispose: () => { disposed.push(key); } });
    put("a"); put("b"); cache.get("a"); put("c");
    expect(disposed).toEqual(["b"]);
    expect(cache.peek("b")).toBeUndefined();
    cache.setBudget(10);
    expect(disposed).toEqual(["b", "a"]);
    cache.clear();
    expect(disposed).toEqual(["b", "a", "c"]);
    expect(cache.usedBytes).toBe(0);
  });

  test("large scroll bursts keep concurrency bounded and cancel queued work", async () => {
    const queue = new ThumbnailWorkQueue(2);
    const controllers = Array.from({ length: 100 }, () => new AbortController());
    const finish: Array<() => void> = [];
    let running = 0, peak = 0, started = 0;
    const promises = controllers.map((controller) => queue.enqueue(async () => {
      running++; started++; peak = Math.max(peak, running);
      await new Promise<void>((resolve) => finish.push(resolve));
      running--;
      return "thumbnail";
    }, () => 1, controller.signal));
    await tick();
    expect(started).toBe(2);
    controllers.slice(2).forEach((controller) => controller.abort());
    finish.splice(0).forEach((resolve) => resolve());
    const results = await Promise.all(promises);
    expect(peak).toBe(2);
    expect(started).toBe(2);
    expect(results.filter((result) => result === null)).toHaveLength(98);
  });

  test("the selected sidebar preview survives scrolling and releases on selection change", () => {
    const cache = new DecodedThumbnailCache(20);
    const put = (key: string) => cache.put(key, { source: key, bytes: 10, dispose() {} });
    const release = cache.retain("selected");
    put("selected"); put("tile-1"); put("tile-2");
    expect(cache.peek("selected")).toBe("selected");
    expect(cache.peek("tile-1")).toBeUndefined();
    expect(cache.usedBytes).toBe(20);
    release(); release();
    put("tile-3");
    expect(cache.peek("selected")).toBeUndefined();
  });

  test("retained sidebar previews still respect a reduced memory budget", () => {
    const cache = new DecodedThumbnailCache(20);
    const release = cache.retain("selected");
    cache.put("selected", { source: "selected", bytes: 10, dispose() {} });
    cache.setBudget(0);
    expect(cache.usedBytes).toBe(0);
    expect(cache.size).toBe(0);
    release();
  });

  test("visible requests jump ahead and high performance increases throughput", async () => {
    const queue = new ThumbnailWorkQueue(1);
    let finish!: () => void;
    const signal = new AbortController().signal;
    const order: string[] = [];
    const active = queue.enqueue(() => new Promise<void>((resolve) => { finish = resolve; }), () => 0, signal);
    await tick();
    const background = queue.enqueue(async () => { order.push("background"); }, () => 2, signal);
    const visible = queue.enqueue(async () => { order.push("visible"); }, () => 0, signal);
    queue.setConcurrency(2);
    await tick();
    expect(order).toEqual(["visible", "background"]);
    finish();
    await Promise.all([active, background, visible]);
  });
});
