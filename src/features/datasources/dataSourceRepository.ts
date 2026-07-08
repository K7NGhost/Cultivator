import Database from "@tauri-apps/plugin-sql";

import type {
  CreateDataSourceInput,
  DataSourceRecord,
  DataSourceType,
} from "@/features/datasources/types";

const databasePromises = new Map<string, Promise<Database>>();
const dataSourcesChangedEvent = "cultivator:datasources-changed";

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

type DataSourcePathCleanupRow = {
  path: string;
};

function createId(prefix: string) {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }

  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function normalizePath(path: string) {
  return path.replace(/\\/g, "/");
}

function createSqliteUrl(databasePath: string) {
  return `sqlite:${normalizePath(databasePath)}`;
}

async function getCaseDatabase(databasePath: string) {
  const normalizedPath = normalizePath(databasePath);

  if (!databasePromises.has(normalizedPath)) {
    databasePromises.set(
      normalizedPath,
      Database.load(createSqliteUrl(normalizedPath)).then(async (database) => {
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

        return database;
      }),
    );
  }

  return databasePromises.get(normalizedPath)!;
}

async function ensureDataSourceDependencyTables(database: Database) {
  await database.execute(`
    CREATE TABLE IF NOT EXISTS plugin_jobs (
      id TEXT PRIMARY KEY,
      case_id TEXT NOT NULL,
      datasource_id TEXT NOT NULL,
      plugin_id TEXT NOT NULL,
      status TEXT NOT NULL,
      started_at TEXT NOT NULL,
      finished_at TEXT,
      error TEXT
    )
  `);

  await database.execute(`
    CREATE TABLE IF NOT EXISTS plugin_results (
      id TEXT PRIMARY KEY,
      job_id TEXT NOT NULL,
      plugin_id TEXT NOT NULL,
      datasource_id TEXT NOT NULL,
      file_path TEXT NOT NULL,
      result_kind TEXT NOT NULL,
      label TEXT NOT NULL,
      payload TEXT NOT NULL,
      created_at TEXT NOT NULL,
      FOREIGN KEY (job_id) REFERENCES plugin_jobs (id)
        ON DELETE CASCADE
    )
  `);

  await database.execute(`
    CREATE TABLE IF NOT EXISTS plugin_logs (
      id TEXT PRIMARY KEY,
      job_id TEXT NOT NULL,
      plugin_id TEXT NOT NULL,
      level TEXT NOT NULL,
      message TEXT NOT NULL,
      created_at TEXT NOT NULL,
      FOREIGN KEY (job_id) REFERENCES plugin_jobs (id)
        ON DELETE CASCADE
    )
  `);

  await database.execute(`
    CREATE TABLE IF NOT EXISTS media_gallery_items (
      id TEXT PRIMARY KEY,
      job_id TEXT NOT NULL,
      datasource_id TEXT NOT NULL,
      media_type TEXT NOT NULL,
      path TEXT NOT NULL,
      name TEXT NOT NULL,
      payload TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      UNIQUE(datasource_id, path),
      FOREIGN KEY (job_id) REFERENCES plugin_jobs (id)
        ON DELETE CASCADE
    )
  `);

  await database.execute(`
    CREATE TABLE IF NOT EXISTS file_tags (
      id TEXT PRIMARY KEY,
      file_path TEXT NOT NULL,
      file_name TEXT NOT NULL,
      file_kind TEXT NOT NULL,
      file_size INTEGER,
      file_modified_ms INTEGER,
      tag_name TEXT NOT NULL,
      tag_group TEXT NOT NULL,
      comment TEXT NOT NULL DEFAULT '',
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      UNIQUE(file_path, tag_name)
    )
  `);
}

function escapeSqlLikePattern(value: string) {
  return value.replace(/[\\%_]/g, (match) => `\\${match}`);
}

function createPathDescendantLikePattern(path: string) {
  return `${escapeSqlLikePattern(normalizePath(path).replace(/\/+$/, ""))}/%`;
}

async function deleteFileTagsForDataSourcePaths(
  database: Database,
  paths: string[],
) {
  for (const path of paths) {
    const normalizedPath = normalizePath(path);

    await database.execute(
      `
        DELETE FROM file_tags
        WHERE file_path = $1
           OR file_path LIKE $2 ESCAPE '\\'
      `,
      [normalizedPath, createPathDescendantLikePattern(normalizedPath)],
    );
  }
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

  const database = await getCaseDatabase(input.caseDatabasePath);
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

  notifyDataSourcesChanged(input.caseId);

  return nextDataSource;
}

export function notifyDataSourcesChanged(caseId: string) {
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
  const database = await getCaseDatabase(input.caseDatabasePath);
  await ensureDataSourceDependencyTables(database);

  const pathRows = await database.select<DataSourcePathCleanupRow[]>(
    `
      SELECT path
      FROM data_source_paths
      WHERE data_source_id = $1
    `,
    [input.dataSourceId],
  );
  const fallbackRows = await database.select<DataSourcePathCleanupRow[]>(
    `
      SELECT path
      FROM data_sources
      WHERE id = $1
        AND case_id = $2
      LIMIT 1
    `,
    [input.dataSourceId, input.caseId],
  );
  const cleanupPaths = Array.from(
    new Set(
      [...pathRows, ...fallbackRows]
        .map((row) => normalizePath(row.path))
        .filter(Boolean),
    ),
  );

  await database.execute("BEGIN IMMEDIATE");

  try {
    // Datasource-owned evidence can be represented in several feature tables.
    // Delete those rows first so the gallery, plugin output, and tags do not
    // keep stale references after the datasource disappears from the tree.
    await deleteFileTagsForDataSourcePaths(database, cleanupPaths);
    await database.execute(
      `
        DELETE FROM media_gallery_items
        WHERE datasource_id = $1
      `,
      [input.dataSourceId],
    );
    await database.execute(
      `
        DELETE FROM plugin_logs
        WHERE job_id IN (
          SELECT id
          FROM plugin_jobs
          WHERE datasource_id = $1
        )
      `,
      [input.dataSourceId],
    );
    await database.execute(
      `
        DELETE FROM plugin_results
        WHERE datasource_id = $1
           OR job_id IN (
             SELECT id
             FROM plugin_jobs
             WHERE datasource_id = $1
           )
      `,
      [input.dataSourceId],
    );
    await database.execute(
      `
        DELETE FROM plugin_jobs
        WHERE datasource_id = $1
      `,
      [input.dataSourceId],
    );
    await database.execute(
      `
        DELETE FROM data_source_plugins
        WHERE data_source_id = $1
      `,
      [input.dataSourceId],
    );
    await database.execute(
      `
        DELETE FROM data_source_paths
        WHERE data_source_id = $1
      `,
      [input.dataSourceId],
    );
    await database.execute(
      `
        DELETE FROM data_sources
        WHERE id = $1
          AND case_id = $2
      `,
      [input.dataSourceId, input.caseId],
    );

    await database.execute("COMMIT");
  } catch (caughtError) {
    await database.execute("ROLLBACK").catch(() => undefined);
    throw caughtError;
  }

  notifyDataSourcesChanged(input.caseId);
}
