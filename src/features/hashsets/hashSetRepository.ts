import { invoke } from "@tauri-apps/api/core";

import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";

export type HashSetKind = "known-bad" | "known-good";

export type HashSetInfo = {
  id: string;
  name: string;
  kind: HashSetKind;
  sourceFile: string;
  /** Milliseconds since 1970, as text. */
  importedAt: string;
  entryCount: number;
};

export type HashSetHit = {
  filePath: string;
  fileSize: number | null;
  fileModifiedMs: number | null;
  setId: string;
  setName: string;
  kind: HashSetKind;
  hash: string;
};

export type HashSetSummary = {
  id: string;
  name: string;
  kind: HashSetKind;
  /** Files in the open case that match the set. */
  count: number;
};

/** Fired after a hash set is imported or deleted, so open pages re-check matches. */
export const HASH_SETS_CHANGED = "cultivator:hash-sets-changed";

export function listHashSets() {
  return invoke<HashSetInfo[]>("list_hash_sets");
}

export function importHashSet(sourcePath: string, name: string, kind: HashSetKind) {
  return invoke<HashSetInfo>("import_hash_set", { sourcePath, name, kind });
}

export function deleteHashSet(id: string) {
  return invoke<void>("delete_hash_set", { id });
}

/** The open case's files whose MD5, SHA-1 or SHA-256 is in a hash set. */
export function matchHashSets(caseDatabasePath: string) {
  return invoke<HashSetHit[]>("match_hash_sets", { caseDatabasePath });
}

/** Every hash set, with how many of the case's files match it. */
export function summarizeHashSetHits(sets: HashSetInfo[], hits: HashSetHit[]): HashSetSummary[] {
  return sets.map((set) => ({
    id: set.id,
    name: set.name,
    kind: set.kind,
    count: new Set(hits.filter((hit) => hit.setId === set.id).map((hit) => hit.filePath)).size,
  }));
}

/** The files matching one hash set, as rows for the file list. */
export function hashSetEntries(hits: HashSetHit[], setId: string): EvidenceDirectoryEntry[] {
  const entries = new Map<string, EvidenceDirectoryEntry>();

  for (const hit of hits) {
    if (hit.setId !== setId || entries.has(hit.filePath)) {
      continue;
    }
    entries.set(hit.filePath, {
      id: `hashset:${setId}:${hit.filePath}`,
      name: hit.filePath.split(/[\\/]/).pop() ?? hit.filePath,
      path: hit.filePath,
      kind: "file",
      size: hit.fileSize ?? undefined,
      modifiedMs: hit.fileModifiedMs ?? undefined,
    });
  }

  return [...entries.values()].sort((left, right) => left.path.localeCompare(right.path));
}
