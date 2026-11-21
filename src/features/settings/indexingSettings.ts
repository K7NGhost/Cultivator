import { useEffect } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

export type IndexingMode = "adaptive" | "background" | "manual";
export interface IndexingOptions { mode: IndexingMode; scanSeconds: number; maxFileMib: number; semantic?: boolean }
export interface IndexingStage { name: string; processed: number; total: number; skipped: number; state: string; checked?: number }
export interface IndexingStatus { database: string | null; options: IndexingOptions; stages: IndexingStage[]; running: boolean; message: string }
export const defaultIndexingOptions: IndexingOptions = { mode: "adaptive", scanSeconds: 60, maxFileMib: 8 };
const key = "cultivator.indexing.v1";
const changed = "cultivator-indexing-options";

export function loadIndexingOptions(): IndexingOptions {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? "null");
    if (value && typeof value === "object") {
      const options = value as IndexingOptions;
      if ((options.semantic === undefined || typeof options.semantic === "boolean") && ["adaptive", "background", "manual"].includes(options.mode) && Number.isInteger(options.scanSeconds) && options.scanSeconds >= 10 && options.scanSeconds <= 3600 && Number.isInteger(options.maxFileMib) && options.maxFileMib >= 1 && options.maxFileMib <= 32) return options;
    }
  } catch { /* Missing or invalid preferences use conservative defaults. */ }
  return { ...defaultIndexingOptions };
}

export function saveIndexingOptions(options: IndexingOptions) {
  localStorage.setItem(key, JSON.stringify(options));
  window.dispatchEvent(new Event(changed));
}

/** One app-level subscription; the native worker outlives individual pages. */
export function useCaseIndexer(database: string | null) {
  useEffect(() => {
    if (!isTauri()) return;
    const configure = () => { void invoke("configure_case_indexer", { database, options: loadIndexingOptions() }).catch(console.error); };
    configure();
    window.addEventListener(changed, configure);
    return () => { window.removeEventListener(changed, configure); };
  }, [database]);

  useEffect(() => {
    if (!database || !isTauri()) return;
    let last = 0;
    const activity = () => {
      if (Date.now() - last < 2000) return;
      last = Date.now();
      void invoke("case_indexer_activity").catch(console.error);
    };
    const events = ["pointerdown", "keydown", "wheel"] as const;
    events.forEach(event => window.addEventListener(event, activity, { passive: true }));
    return () => { events.forEach(event => window.removeEventListener(event, activity)); };
  }, [database]);
}
