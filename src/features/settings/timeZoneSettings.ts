import { useSyncExternalStore } from "react";
import { isTimeZone } from "../../lib/timeZone";

const storageKey = "cultivator.timeZone.v1";
const listeners = new Set<() => void>();
export function getTimeZonePreference(): string {
  try {
    const value = localStorage.getItem(storageKey);
    return value && (value === "system" || isTimeZone(value)) ? value : "UTC";
  } catch { return "UTC"; }
}
export function resolveTimeZone(preference: string): string {
  return preference === "system" ? Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC" : preference;
}
export const getTimeZone = () => resolveTimeZone(getTimeZonePreference());
export function saveTimeZone(preference: string) {
  if (preference !== "system" && !isTimeZone(preference)) throw new Error("Choose a valid timezone.");
  localStorage.setItem(storageKey, preference);
  listeners.forEach((listener) => listener());
}
function subscribe(listener: () => void) {
  listeners.add(listener);
  const onStorage = (event: StorageEvent) => { if (event.key === storageKey || event.key === null) listener(); };
  if (typeof window !== "undefined") window.addEventListener("storage", onStorage);
  return () => { listeners.delete(listener); if (typeof window !== "undefined") window.removeEventListener("storage", onStorage); };
}
export const useTimeZonePreference = () => useSyncExternalStore(subscribe, getTimeZonePreference, () => "UTC");
export const useTimeZone = () => resolveTimeZone(useTimeZonePreference());

export function availableTimeZones(): string[] {
  const intl = Intl as typeof Intl & { supportedValuesOf?: (key: string) => string[] };
  const zones = intl.supportedValuesOf?.("timeZone") ?? ["America/New_York", "America/Chicago", "America/Denver", "America/Los_Angeles",
    "Europe/London", "Europe/Paris", "Asia/Kolkata", "Asia/Tokyo", "Australia/Sydney"];
  return [...new Set(["UTC", getTimeZone(), ...zones])];
}
