const SIDEBAR_STATE_STORAGE_KEY = "cultivator.sidebar.open";

export function loadSidebarOpenState() {
  if (typeof localStorage === "undefined") {
    return true;
  }

  const storedValue = localStorage.getItem(SIDEBAR_STATE_STORAGE_KEY);

  if (storedValue === null) {
    return true;
  }

  return storedValue === "true";
}

export function saveSidebarOpenState(isOpen: boolean) {
  if (typeof localStorage === "undefined") {
    return;
  }

  localStorage.setItem(SIDEBAR_STATE_STORAGE_KEY, String(isOpen));
}
