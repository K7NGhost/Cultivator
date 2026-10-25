import { invoke } from "@tauri-apps/api/core";

import {
  getCaseDatabase,
  normalizePath,
  resetCaseDatabase,
  withCaseDatabaseWriteLock,
} from "@/features/cases";
import type {
  CreateDataSourceInput,
  DataSourceRecord,
  DataSourceType,
} from "../model/types";

const dataSourcesChangedEvent = "cultivator:datasources-changed";
const dataSourceListRequests = new Map<string, Promise<DataSourceRecord[]>>();

type DataSourceRow = {
  id: string;
  case_id: string;
  name: string;
  type: DataSourceType;
  path: string;
  created_at: string;
  original_paths: string | null;
  manifest_path: string | null;
};

type DataSourcePathRow = {
  data_source_id: string;
  path: string;
};

type DataSourcePluginRow = {
  data_source_id: string;
  plugin_id: string;
};

function createId(prefix: string) {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }

  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

async function ensureDataSourceTables(databasePath: string) {
  await withCaseDatabaseWriteLock(databasePath, async () => {
    const database = await getCaseDatabase(databasePath);

    await database.execute(`CREATE TABLE IF NOT EXISTS evidence_sources (
      data_source_id TEXT PRIMARY KEY, original_paths TEXT NOT NULL, manifest_path TEXT NOT NULL,
      FOREIGN KEY (data_source_id) REFERENCES data_sources(id) ON DELETE CASCADE
    )`);

    await database.execute(`
      CREATE TABLE IF NOT EXISTS data_sources (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL,
        name TEXT NOT NULL,
        type TEXT NOT NULL,
        path TEXT NOT NULL,
        created_at TEXT NOT NULL
      )
    `);

    await database.execute(`
      CREATE TABLE IF NOT EXISTS data_source_plugins (
        id TEXT PRIMARY KEY,
        data_source_id TEXT NOT NULL,
        plugin_id TEXT NOT NULL,
        plugin_name TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY (data_source_id) REFERENCES data_sources (id)
          ON DELETE CASCADE
      )
    `);

    await database.execute(`
      CREATE TABLE IF NOT EXISTS data_source_paths (
        id TEXT PRIMARY KEY,
        data_source_id TEXT NOT NULL,
        path TEXT NOT NULL,
        sort_order INTEGER NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY (data_source_id) REFERENCES data_sources (id)
          ON DELETE CASCADE
      )
    `);
  });
}

export async function createDataSource(
  input: CreateDataSourceInput,
): Promise<DataSourceRecord> {
  const name = input.name.trim().replace(/\s+/g, " ");

  if (!name) {
    throw new Error("Datasource name is required.");
  }

  const normalizedPaths = Array.from(
    new Set(input.paths.map(normalizePath).filter(Boolean)),
  );

  if (normalizedPaths.length === 0) {
    throw new Error("Select at least one datasource path.");
  }

  if (input.partition !== undefined && (!Number.isSafeInteger(input.partition) || input.partition < 1)) {
    throw new Error("Partition number must be a positive integer.");
  }
  await ensureDataSourceTables(input.caseDatabasePath);
  const datasourceId = createId("datasource");
  input.onImportStarted?.(datasourceId);
  const imported = await invoke<{ paths: string[]; originalPaths: string[]; manifestPath: string }>("import_evidence", {
    request: { caseFolderPath: input.caseFolderPath, datasourceId, sourceType: input.type,
      paths: normalizedPaths, partition: input.partition ?? null },
  });

  const now = new Date().toISOString();
  const nextDataSource: DataSourceRecord = {
    id: datasourceId,
    caseId: input.caseId,
    name,
    type: input.type,
    path: normalizePath(imported.paths[0]),
    paths: imported.paths.map(normalizePath),
    originalPaths: imported.originalPaths,
    manifestPath: imported.manifestPath,
    pluginIds: input.plugins.map((plugin) => plugin.id),
    createdAt: now,
  };

  try {
    await withCaseDatabaseWriteLock(input.caseDatabasePath, async () => {
      await invoke("commit_evidence_import", {
        caseDatabasePath: input.caseDatabasePath,
        record: { ...nextDataSource, plugins: input.plugins.map((plugin) => ({ id: plugin.id, name: plugin.name })) },
      });
    });
  } catch (error) {
    await invoke("discard_evidence_import", { caseFolderPath: input.caseFolderPath, datasourceId });
    throw error;
  }

  notifyDataSourcesChanged(input.caseId);

  return nextDataSource;
}

export function notifyDataSourcesChanged(caseId: string) {
  for (const cacheKey of dataSourceListRequests.keys()) {
    if (cacheKey.endsWith(`\u0000${caseId}`)) {
      dataSourceListRequests.delete(cacheKey);
    }
  }

  window.dispatchEvent(
    new CustomEvent(dataSourcesChangedEvent, {
      detail: { caseId },
    }),
  );
}

export function subscribeToDataSourcesChanged(
  listener: (caseId: string) => void,
) {
  function handleDataSourcesChanged(event: Event) {
    const caseId =
      event instanceof CustomEvent &&
      typeof event.detail?.caseId === "string"
        ? event.detail.caseId
        : "";

    if (caseId) {
      listener(caseId);
    }
  }

  window.addEventListener(dataSourcesChangedEvent, handleDataSourcesChanged);

  return () => {
    window.removeEventListener(
      dataSourcesChangedEvent,
      handleDataSourcesChanged,
    );
  };
}

export async function listDataSources(
  caseDatabasePath: string,
  caseId: string,
): Promise<DataSourceRecord[]> {
  const cacheKey = `${caseDatabasePath}\u0000${caseId}`;
  const existingRequest = dataSourceListRequests.get(cacheKey);

  if (existingRequest) {
    return existingRequest;
  }

  const request = listDataSourcesUncached(caseDatabasePath, caseId).catch(
    (error) => {
      dataSourceListRequests.delete(cacheKey);
      throw error;
    },
  );
  dataSourceListRequests.set(cacheKey, request);

  return request;
}

async function listDataSourcesUncached(
  caseDatabasePath: string,
  caseId: string,
): Promise<DataSourceRecord[]> {
  await ensureDataSourceTables(caseDatabasePath);

  const database = await getCaseDatabase(caseDatabasePath);
  const rows = await database.select<DataSourceRow[]>(
    `
      SELECT
        d.id, d.case_id, d.name, d.type, d.path, d.created_at,
        e.original_paths, e.manifest_path
      FROM data_sources d
      LEFT JOIN evidence_sources e ON e.data_source_id = d.id
      WHERE d.case_id = $1
      ORDER BY d.created_at DESC
    `,
    [caseId],
  );
  const dataSourceIds = rows.map((row) => row.id);

  if (dataSourceIds.length === 0) {
    return [];
  }

  const pathRows = await database.select<DataSourcePathRow[]>(
    `
      SELECT
        data_source_id,
        path
      FROM data_source_paths
      WHERE data_source_id IN (${dataSourceIds.map(() => "?").join(", ")})
      ORDER BY sort_order ASC
    `,
    dataSourceIds,
  );
  const pluginRows = await database.select<DataSourcePluginRow[]>(
    `
      SELECT
        data_source_id,
        plugin_id
      FROM data_source_plugins
      WHERE data_source_id IN (${dataSourceIds.map(() => "?").join(", ")})
      ORDER BY created_at ASC
    `,
    dataSourceIds,
  );
  const pathsByDataSourceId = new Map<string, string[]>();
  const pluginIdsByDataSourceId = new Map<string, string[]>();

  for (const pathRow of pathRows) {
    pathsByDataSourceId.set(pathRow.data_source_id, [
      ...(pathsByDataSourceId.get(pathRow.data_source_id) ?? []),
      pathRow.path,
    ]);
  }

  for (const pluginRow of pluginRows) {
    pluginIdsByDataSourceId.set(pluginRow.data_source_id, [
      ...(pluginIdsByDataSourceId.get(pluginRow.data_source_id) ?? []),
      pluginRow.plugin_id,
    ]);
  }

  return rows.map((row) => {
    const paths = pathsByDataSourceId.get(row.id) ?? [row.path];

    return {
      id: row.id,
      caseId: row.case_id,
      name: row.name,
      type: row.type,
      path: row.path,
      paths,
      originalPaths: row.original_paths ? JSON.parse(row.original_paths) as string[] : paths,
      manifestPath: row.manifest_path ?? undefined,
      pluginIds: pluginIdsByDataSourceId.get(row.id) ?? [],
      createdAt: row.created_at,
    };
  });
}

export async function removeDataSource(input: {
  caseDatabasePath: string;
  caseId: string;
  dataSourceId: string;
}): Promise<void> {
  await withCaseDatabaseWriteLock(input.caseDatabasePath, async () => {
    await resetCaseDatabase(input.caseDatabasePath);

    await invoke("remove_datasource", {
      caseDatabasePath: input.caseDatabasePath,
      caseId: input.caseId,
      datasourceId: input.dataSourceId,
    });
  });

  notifyDataSourcesChanged(input.caseId);
}
