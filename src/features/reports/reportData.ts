import { invoke } from "@tauri-apps/api/core";

import { listArtifacts } from "@/features/artifacts/artifactRepository";
import { indexAssistantData } from "@/features/assistant/assistantEngine";
import { normalizePath } from "@/features/cases/caseDatabase";
import type { CaseRecord } from "@/features/cases/types";
import { listDataSources } from "@/features/datasources/dataSourceRepository";
import type { DataSourceRecord } from "@/features/datasources/types";
import { countFileHashes, listFileHashesForPaths, type FileHashRecord } from "@/features/files/fileHashRepository";
import { listAllFileTags, type FileTagRecord } from "@/features/files/fileTagRepository";
import { listHashSets, matchHashSets, type HashSetHit } from "@/features/hashsets/hashSetRepository";
import { listMediaGallery } from "@/features/media/mediaRepository";
import { listPluginJobs, listPluginLogs } from "@/features/plugins/pluginRepository";
import type { PluginJobRecord, PluginLogRecord } from "@/features/plugins/types";

export type ReportSections = {
  tagged: boolean;
  hashSetHits: boolean;
  mismatches: boolean;
  processingLog: boolean;
  /** Copy tagged photos next to the report and show them in it. */
  photos: boolean;
};

export type TaggedItem = { tag: FileTagRecord; hash: FileHashRecord | null };

export type ReportData = {
  generatedAt: Date;
  caseRecord: CaseRecord;
  dataSources: DataSourceRecord[];
  counts: {
    hashedFiles: number;
    messages: number;
    calls: number;
    contacts: number;
    photos: number;
    videos: number;
  };
  tagged: TaggedItem[];
  hashSetHits: HashSetHit[];
  hashSetCount: number;
  mismatches: Array<{ name: string; path: string }>;
  jobs: PluginJobRecord[];
  logs: PluginLogRecord[];
};

const MAX_LISTED = 5_000;

/** Everything a case report shows, read from the case and the hash set library. */
export async function collectReportData(caseRecord: CaseRecord): Promise<ReportData> {
  const databasePath = caseRecord.databasePath;
  const [dataSources, artifacts, gallery, hashedFiles, tags, hashSets, hits, jobs, logs] = await Promise.all([
    listDataSources(databasePath, caseRecord.id),
    listArtifacts(databasePath),
    listMediaGallery(databasePath, null),
    countFileHashes(databasePath),
    listAllFileTags(databasePath),
    listHashSets(),
    matchHashSets(databasePath),
    listPluginJobs(databasePath),
    listPluginLogs(databasePath, 1_000),
  ]);
  const index = indexAssistantData(artifacts, [...gallery.photos, ...gallery.videos]);
  const hashes = await listFileHashesForPaths(databasePath, tags.map((tag) => tag.filePath));
  const roots = dataSources.flatMap((source) => (source.paths.length > 0 ? source.paths : [source.path]));
  const mismatchPage =
    roots.length > 0
      ? await invoke<{ entries: Array<{ name: string; path: string }> }>("list_file_view_entries_page", {
          roots,
          viewId: "extension-mismatch",
          offset: 0,
          limit: MAX_LISTED,
        })
      : { entries: [] };

  return {
    generatedAt: new Date(),
    caseRecord,
    dataSources,
    counts: {
      hashedFiles,
      messages: index.messages.length,
      calls: index.calls.length,
      contacts: index.contacts.length,
      photos: gallery.photos.length,
      videos: gallery.videos.length,
    },
    tagged: tags.map((tag) => ({ tag, hash: hashes[tag.filePath] ?? null })),
    hashSetHits: hits,
    hashSetCount: hashSets.length,
    mismatches: mismatchPage.entries.map(({ name, path }) => ({ name, path: normalizePath(path) })),
    jobs,
    logs,
  };
}
