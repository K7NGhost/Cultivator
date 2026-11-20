import { useCallback, useSyncExternalStore } from "react";

export type ArtifactDetailTab = "table" | "preview" | "properties";

const STORAGE_KEY = "cultivator.artifacts.detailTab";
const TABS: readonly ArtifactDetailTab[] = ["table", "preview", "properties"];
const listeners = new Set<() => void>();

function readStoredTab(): ArtifactDetailTab {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    if (value && (TABS as readonly string[]).includes(value)) return value as ArtifactDetailTab;
  } catch {
    // Storage can be unavailable; fall back to the default tab.
  }
  return "table";
}

let currentTab: ArtifactDetailTab = readStoredTab();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot() {
  return currentTab;
}

/** The artifact detail tab (Table / Preview / Properties), shared across artifacts and remembered between sessions. */
export function useArtifactDetailTab() {
  const tab = useSyncExternalStore(subscribe, getSnapshot);
  const setTab = useCallback((value: string) => {
    if (!(TABS as readonly string[]).includes(value) || value === currentTab) return;
    currentTab = value as ArtifactDetailTab;
    try {
      localStorage.setItem(STORAGE_KEY, currentTab);
    } catch {
      // Ignore storage failures; the in-memory value still applies.
    }
    listeners.forEach((listener) => listener());
  }, []);
  return [tab, setTab] as const;
}
