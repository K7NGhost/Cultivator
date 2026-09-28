import { useCallback, useEffect, useState } from "react";

import {
  listDisabledCasePlugins,
  setCasePluginEnabled,
} from "@/features/plugins/casePluginSettings";

function getErrorMessage(caughtError: unknown) {
  return caughtError instanceof Error ? caughtError.message : String(caughtError);
}

/**
 * The open case's plugin on/off switches. Switching a plugin off keeps it
 * installed but stops it being pre-selected for this case's datasources.
 */
export function useCasePluginSwitches(caseDatabasePath: string | null) {
  const [disabledPluginIds, setDisabledPluginIds] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!caseDatabasePath) {
      setDisabledPluginIds(new Set());
      setError(null);
      return;
    }

    try {
      setDisabledPluginIds(await listDisabledCasePlugins(caseDatabasePath));
      setError(null);
    } catch (caughtError) {
      setError(`Could not load this case's plugin switches: ${getErrorMessage(caughtError)}`);
    }
  }, [caseDatabasePath]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function setPluginEnabled(pluginId: string, enabled: boolean) {
    if (!caseDatabasePath) {
      return;
    }

    const previous = disabledPluginIds;
    const next = new Set(previous);
    if (enabled) {
      next.delete(pluginId);
    } else {
      next.add(pluginId);
    }
    setDisabledPluginIds(next);

    try {
      await setCasePluginEnabled(caseDatabasePath, pluginId, enabled);
      setError(null);
    } catch (caughtError) {
      setDisabledPluginIds(previous);
      setError(`Could not save the plugin switch: ${getErrorMessage(caughtError)}`);
    }
  }

  return { disabledPluginIds, error, reload, setPluginEnabled };
}
