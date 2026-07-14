import { invoke } from "@tauri-apps/api/core";

import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";
import type { MediaGalleryResult } from "@/features/media/types";

export type FileViewMediaPage = {
  items: MediaGalleryResult["photos"];
  totalCount: number;
  offset: number;
  limit: number;
  hasNextPage: boolean;
};

type FileViewEntriesPage = {
  entries: EvidenceDirectoryEntry[];
  totalCount: number;
  offset: number;
  limit: number;
  hasNextPage: boolean;
};

export async function listMediaGallery(
  caseDatabasePath: string,
  datasourceId?: string | null,
): Promise<MediaGalleryResult> {
  return invoke<MediaGalleryResult>("list_media_gallery", {
    caseDatabasePath,
    datasourceId,
  });
}

export async function listFileViewMediaPage(
  roots: string[],
  mediaType: "image" | "video",
  offset = 0,
): Promise<FileViewMediaPage> {
  const page = await invoke<FileViewEntriesPage>(
    "list_file_view_entries_page",
    {
      roots,
      viewId:
        mediaType === "image"
          ? "file-types:extension:images"
          : "file-types:extension:videos",
      offset,
      limit: 10_000,
    },
  );

  return {
    items: page.entries.map((entry) => ({
      id: `file-view:${mediaType}:${entry.path}`,
      mediaType,
      name: entry.name,
      path: entry.path,
      format: fileExtension(entry.name),
      size: entry.size ?? 0,
      modifiedMs: entry.modifiedMs ?? null,
      width: null,
      height: null,
      durationMs: null,
      mediaPath: entry.path,
      thumbnailPath: entry.path,
      metadata: [{ label: "Detected by", value: "File extension" }],
    })),
    totalCount: page.totalCount,
    offset: page.offset,
    limit: page.limit,
    hasNextPage: page.hasNextPage,
  };
}

function fileExtension(name: string) {
  const extension = name.match(/\.([^./\\]+)$/)?.[1];
  return extension?.toUpperCase() ?? "Unknown";
}
