import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

export type PerformanceSettings = { memoryMb: number; screensAhead: number; screensBehind: number };
export const defaultPerformanceSettings: PerformanceSettings = { memoryMb: 512, screensAhead: 5, screensBehind: 2 };
const storageKey = "cultivator.performance.v1";
const listeners = new Set<() => void>();
export function normalizePerformanceSettings(value: Partial<PerformanceSettings>): PerformanceSettings {
  const bounded = (n: unknown, fallback: number, min: number, max: number) =>
    typeof n === "number" && Number.isFinite(n) ? Math.max(min, Math.min(max, Math.round(n))) : fallback;
  return {
    memoryMb: bounded(value.memoryMb, 512, 512, 8192),
    screensAhead: bounded(value.screensAhead, 5, 1, 20),
    screensBehind: bounded(value.screensBehind, 2, 1, 10),
  };
}
function readSettings() {
  try { return normalizePerformanceSettings(JSON.parse(localStorage.getItem(storageKey) ?? "{}") ?? {}); }
  catch { return { ...defaultPerformanceSettings }; }
}
let settings = readSettings();
export const getPerformanceSettings = () => settings;
export const subscribePerformanceSettings = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
export const usePerformanceSettings = () => useSyncExternalStore(subscribePerformanceSettings, getPerformanceSettings);
// Split the managed budget between native decoding and browser caches. Reserve
// 10% of the browser portion for gallery records; estimates exclude engine overhead.
export const nativeMemoryMb = (value: PerformanceSettings) => Math.floor(value.memoryMb / 2);
export const thumbnailCacheBytes = (value: PerformanceSettings) => Math.floor(value.memoryMb * 0.45 * 1024 * 1024);
export const recordCacheBytes = (value: PerformanceSettings) => Math.floor(value.memoryMb * 0.05 * 1024 * 1024);
export async function applyPerformanceSettings(value: PerformanceSettings) {
  const next = normalizePerformanceSettings(value);
  // Persist first so a storage failure does not silently discard the preference.
  // Roll back storage if native validation fails.
  const previous = localStorage.getItem(storageKey);
  localStorage.setItem(storageKey, JSON.stringify(next));
  try { await invoke("configure_thumbnail_memory", { memoryMb: nativeMemoryMb(next) }); }
  catch (error) {
    if (previous === null) localStorage.removeItem(storageKey); else localStorage.setItem(storageKey, previous);
    throw error;
  }
  settings = next;
  listeners.forEach((listener) => listener());
}
export async function initializePerformanceSettings() {
  // Preserve environment configuration until the user has saved a preference.
  if (localStorage.getItem(storageKey) !== null) {
    await invoke("configure_thumbnail_memory", { memoryMb: nativeMemoryMb(settings) });
  }
}
