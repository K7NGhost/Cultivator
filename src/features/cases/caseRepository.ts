import { invoke } from "@tauri-apps/api/core";

import {
  getCaseDatabase,
  normalizePath,
  withCaseDatabaseWriteLock,
} from "@/features/cases/caseDatabase";
import type { CaseRecord, CreateCaseInput } from "@/features/cases/types";

const RECENT_CASE_DATABASES_STORAGE_KEY = "cultivator.recentCaseDatabases";

type CaseRow = {
  id: string;
  name: string;
  examiner: string | null;
  reference: string | null;
  description: string | null;
  folder_path: string;
  database_path: string;
  created_at: string;
  updated_at: string;
};

type CaseWorkspacePaths = {
  folderPath: string;
  databasePath: string;
};

function createId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }

  return `case-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function normalizeCaseName(name: string) {
  return name.trim().replace(/\s+/g, " ");
}

function loadRecentCaseDatabasePaths() {
  if (typeof localStorage === "undefined") {
    return [];
  }

  const storedValue = localStorage.getItem(RECENT_CASE_DATABASES_STORAGE_KEY);

  if (!storedValue) {
    return [];
  }

  try {
    const parsedValue = JSON.parse(storedValue);

    if (!Array.isArray(parsedValue)) {
      return [];
    }

    return parsedValue.filter((value): value is string => typeof value === "string");
  } catch {
    return [];
  }
}

function saveRecentCaseDatabasePaths(paths: string[]) {
  if (typeof localStorage === "undefined") {
    return;
  }

  localStorage.setItem(
    RECENT_CASE_DATABASES_STORAGE_KEY,
    JSON.stringify(paths),
  );
}

function rememberCaseDatabasePath(databasePath: string) {
  const normalizedPath = normalizePath(databasePath);
  const nextPaths = [
    normalizedPath,
    ...loadRecentCaseDatabasePaths().filter((path) => path !== normalizedPath),
  ].slice(0, 25);

  saveRecentCaseDatabasePaths(nextPaths);
}

function mapCaseRow(row: CaseRow): CaseRecord {
  return {
    id: row.id,
    name: row.name,
    examiner: row.examiner ?? "",
    reference: row.reference ?? "",
    description: row.description ?? "",
    folderPath: row.folder_path,
    databasePath: row.database_path,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

async function ensureCaseTable(databasePath: string) {
  await withCaseDatabaseWriteLock(databasePath, async () => {
    const database = await getCaseDatabase(databasePath);

    await database.execute(`
      CREATE TABLE IF NOT EXISTS cases (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        examiner TEXT NOT NULL DEFAULT '',
        reference TEXT NOT NULL DEFAULT '',
        description TEXT NOT NULL DEFAULT '',
        folder_path TEXT NOT NULL,
        database_path TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      )
    `);
  });
}

async function loadCase(databasePath: string): Promise<CaseRecord | null> {
  await ensureCaseTable(databasePath);

  const database = await getCaseDatabase(databasePath);
  const rows = await database.select<CaseRow[]>(
    `
      SELECT
        id,
        name,
        examiner,
        reference,
        description,
        folder_path,
        database_path,
        created_at,
        updated_at
      FROM cases
      ORDER BY created_at ASC
      LIMIT 1
    `,
  );

  return rows[0] ? mapCaseRow(rows[0]) : null;
}

export async function listCases(): Promise<CaseRecord[]> {
  const cases = await Promise.all(
    loadRecentCaseDatabasePaths().map(async (databasePath) => {
      try {
        return await loadCase(databasePath);
      } catch {
        return null;
      }
    }),
  );

  return cases.filter((caseRecord): caseRecord is CaseRecord => caseRecord !== null);
}

export async function createCase(input: CreateCaseInput): Promise<CaseRecord> {
  const name = normalizeCaseName(input.name);

  if (!name) {
    throw new Error("Case name is required.");
  }

  if (!input.parentDirectory) {
    throw new Error("Choose where to create the case.");
  }

  const workspacePaths = await invoke<CaseWorkspacePaths>("create_case_workspace", {
    parentDirectory: input.parentDirectory,
    caseName: name,
  });
  const now = new Date().toISOString();
  const normalizedDatabasePath = normalizePath(workspacePaths.databasePath);
  const nextCase: CaseRecord = {
    id: createId(),
    name,
    examiner: input.examiner.trim(),
    reference: input.reference.trim(),
    description: input.description.trim(),
    folderPath: normalizePath(workspacePaths.folderPath),
    databasePath: normalizedDatabasePath,
    createdAt: now,
    updatedAt: now,
  };
  await ensureCaseTable(nextCase.databasePath);

  await withCaseDatabaseWriteLock(nextCase.databasePath, async () => {
    const database = await getCaseDatabase(nextCase.databasePath);

    await database.execute(
      `
        INSERT INTO cases (
          id,
          name,
          examiner,
          reference,
          description,
          folder_path,
          database_path,
          created_at,
          updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
      `,
      [
        nextCase.id,
        nextCase.name,
        nextCase.examiner,
        nextCase.reference,
        nextCase.description,
        nextCase.folderPath,
        nextCase.databasePath,
        nextCase.createdAt,
        nextCase.updatedAt,
      ],
    );
  });

  rememberCaseDatabasePath(nextCase.databasePath);

  return nextCase;
}
