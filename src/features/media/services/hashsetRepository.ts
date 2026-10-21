import { invoke } from "@tauri-apps/api/core";
import type { VicFilterValue } from "../model/vicClassification";
import type { FileViewMediaPage } from "./mediaRepository";

export type MediaHashset = {
  id: string; name: string; hashCount: number; duplicateCount: number; invalidLineCount: number; createdAt: string;
};
export type HashsetRun = {
  id: string; setId: string; datasourceId: string; indexed: boolean;
  status: "queued" | "running" | "complete" | "cancelled" | "interrupted" | "failed";
  scanned: number; hits: number; failed: number; currentPath: string; error: string | null; createdAt: string;
};
export type HashsetFilter = { runId: string; name: string };
export const isHashsetScanActive = (run: HashsetRun) => run.status === "queued" || run.status === "running";

export function listMediaHashsets(caseDatabasePath: string) {
  return invoke<MediaHashset[]>("list_media_hashsets", { caseDatabasePath });
}
export function importMediaHashset(caseDatabasePath: string, filePath: string, name: string) {
  return invoke<MediaHashset>("import_media_hashset", { caseDatabasePath, filePath, name });
}
export function listMediaHashsetRuns(caseDatabasePath: string, datasourceId: string, indexed: boolean) {
  return invoke<HashsetRun[]>("list_media_hashset_runs", { caseDatabasePath, datasourceId, indexed });
}
export function startMediaHashsetScan(caseDatabasePath: string, setId: string, datasourceId: string, indexed: boolean) {
  return invoke<HashsetRun>("start_media_hashset_scan", { caseDatabasePath, setId, datasourceId, indexed });
}
export function cancelMediaHashsetScan(caseDatabasePath: string, runId: string) {
  return invoke<void>("cancel_media_hashset_scan", { caseDatabasePath, runId });
}
export function listHashsetHitMediaPage(caseDatabasePath: string, runId: string, mediaType: "image" | "video", offset = 0, categories: VicFilterValue[] = [], directory: string | null = null) {
  return invoke<FileViewMediaPage>("list_hashset_hit_media_page", { caseDatabasePath, runId, mediaType, offset, categories, directory });
}
