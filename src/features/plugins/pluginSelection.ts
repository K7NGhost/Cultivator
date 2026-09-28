/** The plugins pre-selected for a new datasource: every installed one not switched off. */
export function defaultPluginSelection(
  installedPluginIds: string[],
  disabledPluginIds: ReadonlySet<string>,
): string[] {
  return installedPluginIds.filter((pluginId) => !disabledPluginIds.has(pluginId));
}

/** Adds `pluginIds` to the selection, keeping its order and skipping duplicates. */
export function selectPlugins(selected: string[], pluginIds: string[]): string[] {
  return Array.from(new Set([...selected, ...pluginIds]));
}

/** Removes `pluginIds` from the selection. */
export function deselectPlugins(selected: string[], pluginIds: string[]): string[] {
  const removed = new Set(pluginIds);
  return selected.filter((pluginId) => !removed.has(pluginId));
}

/** Whether every one of `pluginIds` is already selected. */
export function areAllSelected(selected: string[], pluginIds: string[]): boolean {
  const chosen = new Set(selected);
  return pluginIds.length > 0 && pluginIds.every((pluginId) => chosen.has(pluginId));
}
