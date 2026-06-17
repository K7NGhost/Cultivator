export type SearchSettings = {
  binaryFiles: boolean;
  caseSensitive: boolean;
  regex: boolean;
};

const SEARCH_SETTINGS_STORAGE_KEY = "cultivator.search.settings";

const DEFAULT_SEARCH_SETTINGS: SearchSettings = {
  binaryFiles: false,
  caseSensitive: false,
  regex: false,
};

export function loadSearchSettings(): SearchSettings {
  if (typeof localStorage === "undefined") {
    return DEFAULT_SEARCH_SETTINGS;
  }

  const storedValue = localStorage.getItem(SEARCH_SETTINGS_STORAGE_KEY);

  if (!storedValue) {
    return DEFAULT_SEARCH_SETTINGS;
  }

  try {
    return normalizeSearchSettings(JSON.parse(storedValue));
  } catch {
    return DEFAULT_SEARCH_SETTINGS;
  }
}

export function saveSearchSettings(settings: SearchSettings) {
  if (typeof localStorage === "undefined") {
    return;
  }

  localStorage.setItem(
    SEARCH_SETTINGS_STORAGE_KEY,
    JSON.stringify(normalizeSearchSettings(settings)),
  );
}

function normalizeSearchSettings(value: unknown): SearchSettings {
  if (!isSearchSettingsLike(value)) {
    return DEFAULT_SEARCH_SETTINGS;
  }

  return {
    binaryFiles: value.binaryFiles === true,
    caseSensitive: value.caseSensitive === true,
    regex: value.regex === true,
  };
}

function isSearchSettingsLike(value: unknown): value is Partial<SearchSettings> {
  return typeof value === "object" && value !== null;
}
