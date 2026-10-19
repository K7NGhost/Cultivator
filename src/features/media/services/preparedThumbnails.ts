import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useSyncExternalStore } from "react";
import type { MediaItem } from "../model/types";
import type { ThumbnailPriority } from "./mediaRepository";
import { requestThumbnail } from "./thumbnailQueue";
import { DecodedThumbnailCache, decodeThumbnail } from "./decodedThumbnailCache";
import { getPerformanceSettings, subscribePerformanceSettings, thumbnailCacheBytes, thumbnailConcurrency } from "../../settings/performanceSettings";
import { ThumbnailWorkQueue } from "./thumbnailWorkQueue";

export const preparedThumbnails = new DecodedThumbnailCache(thumbnailCacheBytes(getPerformanceSettings()));
const queue = new ThumbnailWorkQueue(thumbnailConcurrency(getPerformanceSettings()));
subscribePerformanceSettings(() => {
  queue.setConcurrency(thumbnailConcurrency(getPerformanceSettings()));
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
export function thumbnailKey(casePath: string | null, item: MediaItem) {
  return [epoch, "native-v1-256", casePath, item.id, item.mediaPath, item.size, item.modifiedMs].join("\u0000");
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
export function prepareThumbnail(casePath: string, item: MediaItem, priority: ThumbnailPriority) {
  const key = thumbnailKey(casePath, item);
  const cached = preparedThumbnails.get(key);
  if (cached) return { promise: Promise.resolve(cached), release() {}, setPriority(_priority: ThumbnailPriority) {} };
  let work = preparing.get(key);
  if (!work) {
    const controller = new AbortController();
    const generation = epoch;
    const current: PreparedWork = { controller, users: new Map(), priority, promise: Promise.resolve(null) };
    preparing.set(key, current);
    current.promise = queue.enqueue(async () => {
      const request = requestThumbnail(key, casePath, item.mediaPath, item.id, current.priority);
      current.updatePriority = request.setPriority;
      const abort = () => request.release();
      controller.signal.addEventListener("abort", abort, { once: true });
      try {
        const path = await request.promise;
        if (!path || controller.signal.aborted || generation !== epoch) return null;
        const value = await decodeThumbnail(convertFileSrc(path), controller.signal);
        if (controller.signal.aborted || generation !== epoch) { value.dispose(); return null; }
        preparedThumbnails.put(key, value);
        return preparedThumbnails.peek(key) ?? null;
      } finally {
        controller.signal.removeEventListener("abort", abort);
        request.release();
      }
    }, () => rank[current.priority], controller.signal).finally(() => {
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
