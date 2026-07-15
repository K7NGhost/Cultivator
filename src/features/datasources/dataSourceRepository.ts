import { invoke } from "@tauri-apps/api/core";

import {
  getCaseDatabase,
  normalizePath,
  resetCaseDatabase,
  withCaseDatabaseWriteLock,
} from "@/features/cases/caseDatabase";
import type {
  CreateDataSourceInput,
  DataSourceRecord,
  DataSourceType,
} from "@/features/datasources/types";

const dataSourcesChangedEvent = "cultivator:datasources-changed";
const dataSourceListRequests = new Map<string, Promise<DataSourceRecord[]>>();

type DataSourceRow = {
  id: string;
  case_id: string;
  name: string;
  type: DataSourceType;
  path: string;
  created_at: string;
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

  const now = new Date().toISOString();
  const nextDataSource: DataSourceRecord = {
    id: createId("datasource"),
    caseId: input.caseId,
    name,
    type: input.type,
    path: normalizedPaths[0],
    paths: normalizedPaths,
    pluginIds: input.plugins.map((plugin) => plugin.id),
    createdAt: now,
  };

  await ensureDataSourceTables(input.caseDatabasePath);

  await withCaseDatabaseWriteLock(input.caseDatabasePath, async () => {
    const database = await getCaseDatabase(input.caseDatabasePath);

    await database.execute(
      `
        INSERT INTO data_sources (
          id,
          case_id,
          name,
          type,
          path,
          created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6)
      `,
      [
        nextDataSource.id,
        nextDataSource.caseId,
        nextDataSource.name,
        nextDataSource.type,
        nextDataSource.path,
        nextDataSource.createdAt,
      ],
    );

    for (const [index, path] of nextDataSource.paths.entries()) {
      await database.execute(
        `
          INSERT INTO data_source_paths (
            id,
            data_source_id,
            path,
            sort_order,
            created_at
          )
          VALUES ($1, $2, $3, $4, $5)
        `,
        [
          createId("datasource-path"),
          nextDataSource.id,
          path,
          index,
          now,
        ],
      );
    }

    for (const plugin of input.plugins) {
      await database.execute(
        `
          INSERT INTO data_source_plugins (
            id,
            data_source_id,
            plugin_id,
            plugin_name,
            created_at
          )
          VALUES ($1, $2, $3, $4, $5)
        `,
        [
          createId("datasource-plugin"),
          nextDataSource.id,
          plugin.id,
          plugin.name,
          now,
        ],
      );
    }
  });

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
        id,
        case_id,
        name,
        type,
        path,
        created_at
      FROM data_sources
      WHERE case_id = $1
      ORDER BY created_at DESC
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
