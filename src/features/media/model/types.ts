import type { VicCategory } from "./vicClassification";

export type MediaGalleryResult = {
  photos: MediaItem[];
  videos: MediaItem[];
  scannedFiles?: number;
};

export type MediaItem = {
  nsfwScore?: number | null;
  faceCount?: number | null;
  faceBoxes?: Array<{ x: number; y: number; width: number; height: number }>;
  clipClassifications?: Array<{ category: number; score: number; matchedLabel: string }>;
  vicCategory?: VicCategory | null;
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
  metadataLoaded?: boolean;
  metadata: Array<{
    label: string;
    value: string;
  }>;
};
