import {
  getCaseDatabase,
  normalizePath,
  withCaseDatabaseWriteLock,
} from "@/features/cases/caseDatabase";

export type FileHashRecord = {
  filePath: string;
  fileSize?: number;
  fileModifiedMs?: number;
  md5: string;
  sha1: string;
  sha256: string;
  hashedAt: string;
};

type FileHashRow = {
  file_path: string;
  file_size: number | null;
  file_modified_ms: number | null;
  md5: string;
  sha1: string;
  sha256: string;
  hashed_at: string;
};

const normalizedDatabases = new Set<string>();

async function ensureFileHashTable(databasePath: string) {
  await withCaseDatabaseWriteLock(databasePath, async () => {
    const database = await getCaseDatabase(databasePath);

    await database.execute(`
      CREATE TABLE IF NOT EXISTS file_hashes (
        id TEXT PRIMARY KEY,
        file_path TEXT NOT NULL,
        file_size INTEGER,
        file_modified_ms INTEGER,
        md5 TEXT NOT NULL,
        sha1 TEXT NOT NULL,
        sha256 TEXT NOT NULL,
        hashed_at TEXT NOT NULL,
        UNIQUE(file_path)
      )
    `);

    await database.execute(`
      CREATE INDEX IF NOT EXISTS idx_file_hashes_file_path
      ON file_hashes(file_path)
    `);

    if (!normalizedDatabases.has(databasePath)) {
      // Earlier builds stored Windows paths with backslashes, which lookups
      // (by normalizePath) never matched. A newer forward-slash row wins.
      await database.execute(`
        DELETE FROM file_hashes
        WHERE instr(file_path, '\\') > 0
          AND REPLACE(file_path, '\\', '/') IN (SELECT file_path FROM file_hashes)
      `);
      await database.execute(`
        UPDATE file_hashes SET file_path = REPLACE(file_path, '\\', '/')
        WHERE instr(file_path, '\\') > 0
      `);
      normalizedDatabases.add(databasePath);
    }
  });
}

function mapHashRow(row: FileHashRow): FileHashRecord {
  return {
    filePath: row.file_path,
    fileSize: row.file_size ?? undefined,
    fileModifiedMs: row.file_modified_ms ?? undefined,
    md5: row.md5,
    sha1: row.sha1,
    sha256: row.sha256,
    hashedAt: row.hashed_at,
  };
}

/** How many files the Hash Calculator has hashed in this case. */
export async function countFileHashes(caseDatabasePath: string): Promise<number> {
  await ensureFileHashTable(caseDatabasePath);

  const database = await getCaseDatabase(caseDatabasePath);
  const rows = await database.select<Array<{ count: number }>>(
    "SELECT COUNT(*) AS count FROM file_hashes",
  );

  return rows[0]?.count ?? 0;
}

export async function listFileHashesForPaths(
  caseDatabasePath: string,
  paths: string[],
): Promise<Record<string, FileHashRecord>> {
  const normalizedPaths = Array.from(new Set(paths.map(normalizePath).filter(Boolean)));

  if (normalizedPaths.length === 0) {
    return {};
  }

  await ensureFileHashTable(caseDatabasePath);

  const database = await getCaseDatabase(caseDatabasePath);
  const rows = await database.select<FileHashRow[]>(
    `
      SELECT
        file_path,
        file_size,
        file_modified_ms,
        md5,
        sha1,
        sha256,
        hashed_at
      FROM file_hashes
      WHERE file_path IN (${normalizedPaths.map(() => "?").join(", ")})
    `,
    normalizedPaths,
  );
  const hashesByPath: Record<string, FileHashRecord> = {};

  for (const row of rows) {
    const hash = mapHashRow(row);

    hashesByPath[hash.filePath] = hash;
  }

  return hashesByPath;
}
