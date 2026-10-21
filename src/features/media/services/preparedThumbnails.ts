import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useSyncExternalStore } from "react";
import type { MediaItem } from "../model/types";
import { readCachedMediaThumbnails, type ThumbnailPriority } from "./mediaRepository";
import { requestThumbnail } from "./thumbnailQueue";
import { DecodedThumbnailCache, decodeThumbnail } from "./decodedThumbnailCache";
import { getPerformanceSettings, subscribePerformanceSettings, thumbnailCacheBytes, thumbnailConcurrency } from "../../settings/performanceSettings";
import { ThumbnailWorkQueue } from "./thumbnailWorkQueue";
import { CachedThumbnailBatcher } from "./cachedThumbnailBatcher";

export const preparedThumbnails = new DecodedThumbnailCache(thumbnailCacheBytes(getPerformanceSettings()));
const queue = new ThumbnailWorkQueue(thumbnailConcurrency(getPerformanceSettings()));
const decodeQueue = new ThumbnailWorkQueue(thumbnailConcurrency(getPerformanceSettings()));
// Small bounded batches keep obsolete scroll ranges from occupying the native
// fingerprint workers while the next viewport waits behind them.
const diskCache = new CachedThumbnailBatcher(readCachedMediaThumbnails, 32, 2);
subscribePerformanceSettings(() => {
  queue.setConcurrency(thumbnailConcurrency(getPerformanceSettings()));
  decodeQueue.setConcurrency(thumbnailConcurrency(getPerformanceSettings()));
  clearPreparedThumbnails();
  preparedThumbnails.setBudget(thumbnailCacheBytes(getPerformanceSettings()));
});
let epoch = 0;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
export const useThumbnailEpoch = () => useSyncExternalStore(subscribe, () => epoch);
export function usePreparedThumbnail(key: string) {
  const subscribeToKey = useCallback((listener: () => void) => preparedThumbnails.subscribe(key, listener), [key]);
  const snapshot = useCallback(() => preparedThumbnails.peek(key), [key]);
  return useSyncExternalStore(subscribeToKey, snapshot);
}
export function clearPreparedThumbnails() {
  epoch++;
  for (const work of preparing.values()) work.controller.abort();
  preparing.clear();
  preparedThumbnails.clear();
  listeners.forEach((listener) => listener());
}
export function thumbnailKey(casePath: string | null, item: MediaItem, edge = 256) {
  return [epoch, "sqlite-v1-256", edge, casePath, item.id, item.mediaPath, item.size, item.modifiedMs].join("\u0000");
}
type PreparedWork = {
  controller: AbortController;
  users: Map<symbol, ThumbnailPriority>;
  priority: ThumbnailPriority;
  updatePriority?: (priority: ThumbnailPriority) => void;
  promise: Promise<string | null>;
};
const preparing = new Map<string, PreparedWork>();
const rank = { visible: 0, nearby: 1, background: 2 };
export function prepareThumbnail(casePath: string, item: MediaItem, priority: ThumbnailPriority, edge = 256) {
  const key = thumbnailKey(casePath, item, edge);
  const cached = preparedThumbnails.get(key);
  if (cached) return { promise: Promise.resolve(cached), release() {}, setPriority(_priority: ThumbnailPriority) {} };
  let work = preparing.get(key);
  if (!work) {
    const controller = new AbortController();
    const generation = epoch;
    const current: PreparedWork = { controller, users: new Map(), priority, promise: Promise.resolve(null) };
    preparing.set(key, current);
    const decode = (path: string) => decodeQueue.enqueue(async () => {
      if (controller.signal.aborted || generation !== epoch) return null;
      const value = await decodeThumbnail(convertFileSrc(path, "thumbnail"), controller.signal, edge);
      if (controller.signal.aborted || generation !== epoch) { value.dispose(); return null; }
      preparedThumbnails.put(key, value);
      return preparedThumbnails.peek(key) ?? null;
    }, () => rank[current.priority], controller.signal);
    current.promise = diskCache.read(casePath, item.mediaPath, controller.signal, () => rank[current.priority]).then(path => {
      if (controller.signal.aborted || generation !== epoch) return null;
      if (path) return decode(path);
      return queue.enqueue(async () => {
        const request = requestThumbnail(key, casePath, item.mediaPath, item.id, current.priority);
        current.updatePriority = request.setPriority;
        const abort = () => request.release();
        controller.signal.addEventListener("abort", abort, { once: true });
        try {
          const path = await request.promise;
          if (!path || controller.signal.aborted || generation !== epoch) return null;
          return await decode(path);
        } finally {
          controller.signal.removeEventListener("abort", abort);
          request.release();
        }
      }, () => rank[current.priority], controller.signal);
    }).finally(() => {
      if (preparing.get(key) === current) preparing.delete(key);
    });
    work = current;
  }
  const current = work;
  const user = Symbol();
  current.users.set(user, priority);
  const update = () => {
    const next = [...current.users.values()].sort((a, b) => rank[a] - rank[b])[0];
    if (!next) {
      if (preparing.get(key) === current) preparing.delete(key);
      current.controller.abort();
    } else {
      current.priority = next;
      current.updatePriority?.(next);
    }
  };
  update();
  return {
    promise: current.promise,
    release() { if (current.users.delete(user)) update(); },
    setPriority(next: ThumbnailPriority) { if (current.users.has(user)) { current.users.set(user, next); update(); } },
  };
}
