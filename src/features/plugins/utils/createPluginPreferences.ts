export type CreatePluginModePreference = "manual" | "automatic";

const createPluginModeStorageKey = "cultivator.createPluginMode";

export function getStoredCreatePluginMode(): CreatePluginModePreference {
  if (typeof window === "undefined") {
    return "manual";
  }

  let storedMode: string | null = null;

  try {
    storedMode = window.localStorage.getItem(createPluginModeStorageKey);
  } catch {
    return "manual";
  }

  return storedMode === "automatic" ? "automatic" : "manual";
}

export function storeCreatePluginMode(mode: CreatePluginModePreference) {
  try {
    window.localStorage.setItem(createPluginModeStorageKey, mode);
  } catch {
    // Losing this preference should not block plugin creation.
  }
}
