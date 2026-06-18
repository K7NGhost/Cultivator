import Database from "@tauri-apps/plugin-sql";

import type {
  CreateDataSourceInput,
  DataSourceRecord,
} from "@/features/datasources/types";

const databasePromises = new Map<string, Promise<Database>>();

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

  return nextDataSource;
}
