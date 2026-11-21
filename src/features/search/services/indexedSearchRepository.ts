import { invoke } from "@tauri-apps/api/core";

export type IndexCategory = "all" | "0" | "1" | "2";
export interface IndexedHit { group: string; stage: string; source: string; body: string; chunk: number }
export interface IndexedSearchResult { ready: boolean; total: number; hits: IndexedHit[] }
export const indexedPageSize = 50;
export const indexedMaxOffset = 10_000;
export function indexedCategoryName(stage: string) {
  return ({ "0": "Filesystem metadata", "1": "Artifacts", "2": "File contents" } as Record<string, string>)[stage] ?? "Indexed record";
}
export function searchCaseIndex(database: string, query: string, category: IndexCategory, offset: number) {
  return invoke<IndexedSearchResult>("search_case_index", { request: { database, query, stage: category === "all" ? null : Number(category), offset, limit: indexedPageSize } });
}
