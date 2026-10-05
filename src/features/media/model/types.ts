export type MediaGalleryResult = {
  photos: MediaItem[];
  videos: MediaItem[];
  scannedFiles?: number;
};

export type MediaItem = {
  id: string;
  mediaType: "image" | "video";
  name: string;
  path: string;
  format: string;
  size: number;
  modifiedMs?: number | null;
  width?: number | null;
  height?: number | null;
  durationMs?: number | null;
  mediaPath: string;
  thumbnailPath: string;
  metadata: Array<{
    label: string;
    value: string;
  }>;
};
