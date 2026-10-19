import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useSyncExternalStore } from "react";
import type { MediaItem } from "../model/types";
import type { ThumbnailPriority } from "./mediaRepository";
import { requestThumbnail } from "./thumbnailQueue";
import { DecodedThumbnailCache, decodeThumbnail } from "./decodedThumbnailCache";
import { getPerformanceSettings, subscribePerformanceSettings, thumbnailCacheBytes } from "../../settings/performanceSettings";

export const preparedThumbnails = new DecodedThumbnailCache(thumbnailCacheBytes(getPerformanceSettings()));
subscribePerformanceSettings(() => preparedThumbnails.setBudget(thumbnailCacheBytes(getPerformanceSettings())));
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
  preparedThumbnails.clear();
  listeners.forEach((listener) => listener());
}
export function thumbnailKey(casePath: string | null, item: MediaItem) {
  return [epoch, "native-v1-256", casePath, item.id, item.mediaPath, item.size, item.modifiedMs].join("\u0000");
}
const decoding = new Map<string, Promise<string | null>>();
export function prepareThumbnail(casePath: string, item: MediaItem, priority: ThumbnailPriority) {
  const key = thumbnailKey(casePath, item);
  const cached = preparedThumbnails.get(key);
  if (cached) return { promise: Promise.resolve(cached), release() {}, setPriority(_priority: ThumbnailPriority) {} };
  const inDecode = decoding.get(key);
  if (inDecode) return { promise: inDecode, release() {}, setPriority(_priority: ThumbnailPriority) {} };
  const request = requestThumbnail(key, casePath, item.mediaPath, item.id, priority);
  const generation = epoch;
  const promise = request.promise.then((path) => {
    if (!path || generation !== epoch) return null;
    const cached = preparedThumbnails.get(key);
    if (cached) return cached;
    let work = decoding.get(key);
    if (!work) {
      work = decodeThumbnail(convertFileSrc(path)).then((value) => {
        if (generation !== epoch) { value.dispose(); return null; }
        preparedThumbnails.put(key, value);
        return value.source;
      }).finally(() => { decoding.delete(key); });
      decoding.set(key, work);
    }
    return work;
  });
  return { promise, release: request.release, setPriority: request.setPriority };
}
