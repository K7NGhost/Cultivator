import { invoke } from "@tauri-apps/api/core";

import type { EvidenceDirectoryEntry } from "@/features/evidence";
import type { MediaGalleryResult } from "../model/types";

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

export async function renderMediaThumbnail(
  caseDatabasePath: string,
  sourcePath: string,
  mediaId: string,
  requestId: string,
  priority: ThumbnailPriority,
): Promise<ThumbnailResult> {
  return invoke<ThumbnailResult>("render_media_thumbnail", {
    caseDatabasePath,
    sourcePath,
    mediaId,
    requestId,
    priority,
  });
}

export type ThumbnailPriority = "visible" | "nearby" | "background";
export type ThumbnailResult = {
  path: string;
  timings: {
    inspectMs: number;
    queueMs: number; startupMs: number; decodeMs: number; resizeMs: number;
    encodeMs: number; writeMs: number; totalMs: number; cacheHit: boolean; backend: string;
  };
};

export function updateMediaThumbnail(requestId: string, priority: ThumbnailPriority | null) {
  return invoke<void>("update_media_thumbnail", { requestId, priority });
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
      limit: 500,
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

export async function listMediaGalleryPage(
  caseDatabasePath: string, datasourceId: string,
  mediaType: "image" | "video", offset = 0,
): Promise<FileViewMediaPage> {
  return invoke<FileViewMediaPage>("list_media_gallery_page", {
    caseDatabasePath, datasourceId, mediaType, offset, limit: 500,
  });
}

export async function loadMediaDetails(
  caseDatabasePath: string, datasourceId: string, sourcePath: string,
): Promise<MediaGalleryResult["photos"][number]> {
  return invoke("load_media_details", { caseDatabasePath, datasourceId, sourcePath });
}
