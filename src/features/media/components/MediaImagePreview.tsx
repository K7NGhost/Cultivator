import { convertFileSrc } from "@tauri-apps/api/core";
import { ImageOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { renderMediaThumbnail } from "@/features/media/mediaRepository";
import type { MediaItem } from "@/features/media/types";
import { cn } from "@/lib/utils";

const convertedPreviewPaths = new Map<string, string>();
const failedPreviewKeys = new Set<string>();
const pendingPreviewRequests = new Map<string, Promise<string>>();

type MediaImagePreviewProps = {
  caseDatabasePath: string | null;
  className?: string;
  item: MediaItem;
};

export function MediaImagePreview({
  caseDatabasePath,
  className,
  item,
}: MediaImagePreviewProps) {
  const previewKey = createPreviewKey(caseDatabasePath, item);
  const requestGeneration = useRef(0);
  const [source, setSource] = useState(() => initialSource(previewKey, item));
  const [fallbackAttempted, setFallbackAttempted] = useState(() =>
    convertedPreviewPaths.has(previewKey),
  );
  const [failed, setFailed] = useState(() => failedPreviewKeys.has(previewKey));

  useEffect(() => {
    requestGeneration.current += 1;
    setSource(initialSource(previewKey, item));
    setFallbackAttempted(convertedPreviewPaths.has(previewKey));
    setFailed(failedPreviewKeys.has(previewKey));
  }, [item, previewKey]);

  async function handleLoadError() {
    if (fallbackAttempted || !caseDatabasePath) {
      failedPreviewKeys.add(previewKey);
      setFailed(true);
      return;
    }

    setFallbackAttempted(true);
    const generation = requestGeneration.current;

    try {
      const previewPath = await getConvertedPreview(
        previewKey,
        caseDatabasePath,
        item.mediaPath,
      );

      if (generation === requestGeneration.current) {
        setSource(convertFileSrc(previewPath));
      }
    } catch {
      failedPreviewKeys.add(previewKey);
      if (generation === requestGeneration.current) {
        setFailed(true);
      }
    }
  }

  if (failed) {
    return (
      <div
        className={cn(
          "flex items-center justify-center bg-muted text-muted-foreground",
          className,
        )}
        aria-label={`${item.name} preview unavailable`}
      >
        <ImageOff className="size-5" aria-hidden="true" />
      </div>
    );
  }

  return (
    <img
      src={source}
      alt={item.name}
      className={className}
      draggable={false}
      loading="lazy"
      onError={() => void handleLoadError()}
    />
  );
}

function initialSource(previewKey: string, item: MediaItem) {
  const convertedPath = convertedPreviewPaths.get(previewKey);
  return convertFileSrc(convertedPath ?? item.thumbnailPath ?? item.mediaPath);
}

function createPreviewKey(caseDatabasePath: string | null, item: MediaItem) {
  return [
    caseDatabasePath ?? "no-case",
    item.mediaPath,
    item.size,
    item.modifiedMs ?? "unknown",
  ].join("\u0000");
}

function getConvertedPreview(
  previewKey: string,
  caseDatabasePath: string,
  sourcePath: string,
) {
  const existingPath = convertedPreviewPaths.get(previewKey);
  if (existingPath) {
    return Promise.resolve(existingPath);
  }

  const existingRequest = pendingPreviewRequests.get(previewKey);
  if (existingRequest) {
    return existingRequest;
  }

  const request = renderMediaThumbnail(caseDatabasePath, sourcePath)
    .then((previewPath) => {
      convertedPreviewPaths.set(previewKey, previewPath);
      return previewPath;
    })
    .finally(() => {
      pendingPreviewRequests.delete(previewKey);
    });

  pendingPreviewRequests.set(previewKey, request);
  return request;
}
