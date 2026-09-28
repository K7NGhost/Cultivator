import {
  getCaseDatabase,
  withCaseDatabaseWriteLock,
} from "@/features/cases/caseDatabase";

/**
 * Per-case plugin switches. Plugins are installed once for the whole app and
 * are all on by default; a case can switch individual plugins off so they are
 * not pre-selected when a datasource is added to it. Stored in the case
 * database, so the choice travels with the case folder.
 */

type CasePluginSettingRow = {
  plugin_id: string;
  enabled: number;
};

async function ensureCasePluginSettingsTable(databasePath: string) {
  await withCaseDatabaseWriteLock(databasePath, async () => {
    const database = await getCaseDatabase(databasePath);

    await database.execute(`
      CREATE TABLE IF NOT EXISTS case_plugin_settings (
        plugin_id TEXT PRIMARY KEY,
        enabled INTEGER NOT NULL,
        updated_at TEXT NOT NULL
      )
    `);
  });
}

/** Plugin IDs switched off for this case. */
export async function listDisabledCasePlugins(databasePath: string): Promise<Set<string>> {
  await ensureCasePluginSettingsTable(databasePath);
  const database = await getCaseDatabase(databasePath);
  const rows = await database.select<CasePluginSettingRow[]>(
    "SELECT plugin_id, enabled FROM case_plugin_settings WHERE enabled = 0",
  );

  return new Set(rows.map((row) => row.plugin_id));
}

/** Switches one plugin on or off for this case. */
export async function setCasePluginEnabled(
  databasePath: string,
  pluginId: string,
  enabled: boolean,
): Promise<void> {
  await setCasePluginStates(databasePath, [{ pluginId, enabled }]);
}

/**
 * Remembers a datasource's plugin choice as this case's default: every listed
 * plugin that was not chosen is switched off, every chosen one on.
 */
export async function rememberCasePluginSelection(
  databasePath: string,
  listedPluginIds: string[],
  chosenPluginIds: string[],
): Promise<void> {
  const chosen = new Set(chosenPluginIds);

  await setCasePluginStates(
    databasePath,
    listedPluginIds.map((pluginId) => ({ pluginId, enabled: chosen.has(pluginId) })),
  );
}

async function setCasePluginStates(
  databasePath: string,
  states: Array<{ pluginId: string; enabled: boolean }>,
) {
  if (states.length === 0) {
    return;
  }

  await ensureCasePluginSettingsTable(databasePath);
  const updatedAt = new Date().toISOString();

  await withCaseDatabaseWriteLock(databasePath, async () => {
    const database = await getCaseDatabase(databasePath);

    for (const state of states) {
      await database.execute(
        `
          INSERT INTO case_plugin_settings (plugin_id, enabled, updated_at)
          VALUES ($1, $2, $3)
          ON CONFLICT (plugin_id) DO UPDATE SET
            enabled = excluded.enabled,
            updated_at = excluded.updated_at
        `,
        [state.pluginId, state.enabled ? 1 : 0, updatedAt],
      );
    }
  });
}
