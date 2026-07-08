import Database from "@tauri-apps/plugin-sql";

const SQLITE_BUSY_TIMEOUT_MS = 5000;

const databasePromises = new Map<string, Promise<Database>>();
const writeQueues = new Map<string, Promise<void>>();

export function normalizePath(path: string) {
  return path.replace(/\\/g, "/");
}

function createSqliteUrl(databasePath: string) {
  return `sqlite:${normalizePath(databasePath)}`;
}

export async function getCaseDatabase(databasePath: string) {
  const normalizedPath = normalizePath(databasePath);

  if (!databasePromises.has(normalizedPath)) {
    databasePromises.set(
      normalizedPath,
      Database.load(createSqliteUrl(normalizedPath)).then(async (database) => {
        await database.execute(`PRAGMA busy_timeout = ${SQLITE_BUSY_TIMEOUT_MS}`);
        await database.execute("PRAGMA journal_mode = WAL");

        return database;
      }),
    );
  }

  return databasePromises.get(normalizedPath)!;
}

export async function resetCaseDatabase(databasePath: string): Promise<void> {
  const normalizedPath = normalizePath(databasePath);
  const sqliteUrl = createSqliteUrl(normalizedPath);

  databasePromises.delete(normalizedPath);

  await Database.get(sqliteUrl)
    .close(sqliteUrl)
    .catch(() => undefined);
}

export async function withCaseDatabaseWriteLock<T>(
  databasePath: string,
  operation: () => Promise<T>,
): Promise<T> {
  const normalizedPath = normalizePath(databasePath);
  const previousWrite = writeQueues.get(normalizedPath) ?? Promise.resolve();
  let releaseWrite: () => void = () => undefined;
  const currentWrite = new Promise<void>((resolve) => {
    releaseWrite = resolve;
  });
  const queuedWrite = previousWrite.catch(() => undefined).then(() => currentWrite);

  writeQueues.set(normalizedPath, queuedWrite);

  await previousWrite.catch(() => undefined);

  try {
    return await operation();
  } finally {
    releaseWrite();

    if (writeQueues.get(normalizedPath) === queuedWrite) {
      writeQueues.delete(normalizedPath);
    }
  }
}
