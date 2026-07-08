import Database from "@tauri-apps/plugin-sql";

import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";

export type FileTagGroup = "bookmark" | "follow-up" | "notable" | "project-vic" | "custom";

export type FileTagRecord = {
  id: string;
  filePath: string;
  fileName: string;
  fileKind: EvidenceDirectoryEntry["kind"];
  fileSize?: number;
  fileModifiedMs?: number;
  tagName: string;
  tagGroup: FileTagGroup;
  comment: string;
  createdAt: string;
  updatedAt: string;
};

export type FileTagSummary = {
  tagName: string;
  tagGroup: FileTagGroup;
  count: number;
};

type FileTagRow = {
  id: string;
  file_path: string;
  file_name: string;
  file_kind: EvidenceDirectoryEntry["kind"];
  file_size: number | null;
  file_modified_ms: number | null;
  tag_name: string;
  tag_group: FileTagGroup;
  comment: string | null;
  created_at: string;
  updated_at: string;
};

type FileTagSummaryRow = {
  tag_name: string;
  tag_group: FileTagGroup;
  count: number;
};

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

        await database.execute(`
          CREATE INDEX IF NOT EXISTS idx_file_tags_file_path
          ON file_tags(file_path)
        `);

        await database.execute(`
          CREATE INDEX IF NOT EXISTS idx_file_tags_tag_name
          ON file_tags(tag_name)
        `);

        return database;
      }),
    );
  }

  return databasePromises.get(normalizedPath)!;
}

function mapTagRow(row: FileTagRow): FileTagRecord {
  return {
    id: row.id,
    filePath: row.file_path,
    fileName: row.file_name,
    fileKind: row.file_kind,
    fileSize: row.file_size ?? undefined,
    fileModifiedMs: row.file_modified_ms ?? undefined,
    tagName: row.tag_name,
    tagGroup: row.tag_group,
    comment: row.comment ?? "",
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export async function listFileTagsForPaths(
  caseDatabasePath: string,
  paths: string[],
): Promise<Record<string, FileTagRecord[]>> {
  const normalizedPaths = Array.from(new Set(paths.map(normalizePath).filter(Boolean)));

  if (normalizedPaths.length === 0) {
    return {};
  }

  const database = await getCaseDatabase(caseDatabasePath);
  const rows = await database.select<FileTagRow[]>(
    `
      SELECT
        id,
        file_path,
        file_name,
        file_kind,
        file_size,
        file_modified_ms,
        tag_name,
        tag_group,
        comment,
        created_at,
        updated_at
      FROM file_tags
      WHERE file_path IN (${normalizedPaths.map(() => "?").join(", ")})
      ORDER BY tag_group ASC, tag_name ASC
    `,
    normalizedPaths,
  );
  const tagsByPath: Record<string, FileTagRecord[]> = {};

  for (const row of rows) {
    const tag = mapTagRow(row);

    tagsByPath[tag.filePath] = [...(tagsByPath[tag.filePath] ?? []), tag];
  }

  return tagsByPath;
}

export async function listFileTagSummaries(
  caseDatabasePath: string,
): Promise<FileTagSummary[]> {
  const database = await getCaseDatabase(caseDatabasePath);
  const rows = await database.select<FileTagSummaryRow[]>(`
    SELECT
      tag_name,
      tag_group,
      COUNT(*) AS count
    FROM file_tags
    GROUP BY tag_name, tag_group
    ORDER BY tag_group ASC, tag_name ASC
  `);

  return rows.map((row) => ({
    tagName: row.tag_name,
    tagGroup: row.tag_group,
    count: row.count,
  }));
}

export async function listTaggedFileEntries(
  caseDatabasePath: string,
  tagName: string,
): Promise<EvidenceDirectoryEntry[]> {
  const database = await getCaseDatabase(caseDatabasePath);
  const rows = await database.select<FileTagRow[]>(
    `
      SELECT
        id,
        file_path,
        file_name,
        file_kind,
        file_size,
        file_modified_ms,
        tag_name,
        tag_group,
        comment,
        created_at,
        updated_at
      FROM file_tags
      WHERE tag_name = $1
      ORDER BY file_path ASC
    `,
    [tagName],
  );

  return rows.map((row) => ({
    id: `tag:${row.tag_name}:${row.file_path}`,
    name: row.file_name,
    path: row.file_path,
    kind: row.file_kind,
    size: row.file_size ?? undefined,
    modifiedMs: row.file_modified_ms ?? undefined,
  }));
}

export async function upsertFileTag(input: {
  caseDatabasePath: string;
  entry: EvidenceDirectoryEntry;
  tagName: string;
  tagGroup: FileTagGroup;
  comment?: string;
}): Promise<void> {
  const tagName = input.tagName.trim().replace(/\s+/g, " ");

  if (!tagName) {
    throw new Error("Tag name is required.");
  }

  const database = await getCaseDatabase(input.caseDatabasePath);
  const now = new Date().toISOString();
  const filePath = normalizePath(input.entry.path);

  await database.execute(
    `
      INSERT INTO file_tags (
        id,
        file_path,
        file_name,
        file_kind,
        file_size,
        file_modified_ms,
        tag_name,
        tag_group,
        comment,
        created_at,
        updated_at
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
      ON CONFLICT(file_path, tag_name) DO UPDATE SET
        file_name = excluded.file_name,
        file_kind = excluded.file_kind,
        file_size = excluded.file_size,
        file_modified_ms = excluded.file_modified_ms,
        tag_group = excluded.tag_group,
        comment = excluded.comment,
        updated_at = excluded.updated_at
    `,
    [
      createId("file-tag"),
      filePath,
      input.entry.name,
      input.entry.kind,
      input.entry.size ?? null,
      input.entry.modifiedMs ?? null,
      tagName,
      input.tagGroup,
      input.comment?.trim() ?? "",
      now,
      now,
    ],
  );
}

export async function removeFileTag(input: {
  caseDatabasePath: string;
  filePath: string;
  tagName: string;
}): Promise<void> {
  const database = await getCaseDatabase(input.caseDatabasePath);

  await database.execute(
    `
      DELETE FROM file_tags
      WHERE file_path = $1
        AND tag_name = $2
    `,
    [normalizePath(input.filePath), input.tagName],
  );
}
