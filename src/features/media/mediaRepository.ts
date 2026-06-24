import { invoke } from "@tauri-apps/api/core";

import type { MediaGalleryResult } from "@/features/media/types";

export async function listMediaGallery(
  caseDatabasePath: string,
  datasourceId?: string | null,
): Promise<MediaGalleryResult> {
  return invoke<MediaGalleryResult>("list_media_gallery", {
    caseDatabasePath,
    datasourceId,
  });
}
