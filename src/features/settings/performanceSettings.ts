import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

export type PerformanceSettings = { memoryMb: number; screensAhead: number; screensBehind: number };
export const performancePresets = {
  "Memory saver": { memoryMb: 384, screensAhead: 1, screensBehind: 0 },
  Balanced: { memoryMb: 768, screensAhead: 3, screensBehind: 1 },
  "High performance": { memoryMb: 2048, screensAhead: 5, screensBehind: 2 },
} satisfies Record<string, PerformanceSettings>;
export function automaticPerformanceSettings(deviceMemory?: number): PerformanceSettings {
  // WebViews may omit the hardware hint. Start conservatively in that case.
  return { ...(deviceMemory && deviceMemory >= 8 ? performancePresets.Balanced : performancePresets["Memory saver"]) };
}
export const defaultPerformanceSettings = automaticPerformanceSettings(
  typeof navigator === "undefined" ? undefined : (navigator as Navigator & { deviceMemory?: number }).deviceMemory,
);
const storageKey = "cultivator.performance.v1";
const listeners = new Set<() => void>();
export function normalizePerformanceSettings(value: Partial<PerformanceSettings>): PerformanceSettings {
  const bounded = (n: unknown, fallback: number, min: number, max: number) =>
    typeof n === "number" && Number.isFinite(n) ? Math.max(min, Math.min(max, Math.round(n))) : fallback;
  return {
    memoryMb: bounded(value.memoryMb, defaultPerformanceSettings.memoryMb, 192, 8192),
    screensAhead: bounded(value.screensAhead, defaultPerformanceSettings.screensAhead, 0, 20),
    screensBehind: bounded(value.screensBehind, defaultPerformanceSettings.screensBehind, 0, 10),
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
// Leave headroom for WebView/GPU copies, encoded blobs, and in-flight work.
export const nativeMemoryMb = (value: PerformanceSettings) => Math.min(4096, Math.floor(value.memoryMb * 2 / 3));
export const thumbnailCacheBytes = (value: PerformanceSettings) => Math.floor(value.memoryMb / 12) * 1024 * 1024;
export const recordCacheBytes = (value: PerformanceSettings) => Math.floor(value.memoryMb / 48) * 1024 * 1024;
export const thumbnailConcurrency = (value: PerformanceSettings) => value.memoryMb <= 512 ? 2 : value.memoryMb <= 1024 ? 4 : 8;
export const galleryOverscanScreens = (value: PerformanceSettings) => value.memoryMb <= 512 ? 0 : value.memoryMb <= 1024 ? 1 : 2;
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
  await invoke("configure_thumbnail_memory", { memoryMb: nativeMemoryMb(settings) });
}
