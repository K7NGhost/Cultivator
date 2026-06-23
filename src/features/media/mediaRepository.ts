import { invoke } from "@tauri-apps/api/core";

import type { MediaGalleryResult } from "@/features/media/types";

export async function listMediaGallery(
  caseDatabasePath: string,
): Promise<MediaGalleryResult> {
  return invoke<MediaGalleryResult>("list_media_gallery", {
    caseDatabasePath,
  });
}
