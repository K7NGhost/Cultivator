import { ImageOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { preparedThumbnails, prepareThumbnail, thumbnailKey, usePreparedThumbnail, useThumbnailEpoch } from "../services/preparedThumbnails";
import type { MediaItem } from "../model/types";
import { cn } from "@/lib/utils";

type MediaImagePreviewProps = {
  caseDatabasePath: string | null;
  className?: string;
  fit?: "cover" | "contain";
  retain?: boolean;
  edge?: number;
  item: MediaItem;
};

export function MediaImagePreview({ caseDatabasePath, className, fit = "cover", retain = false, edge = 256, item }: MediaImagePreviewProps) {
  const container = useRef<HTMLDivElement>(null);
  const [failedKey, setFailedKey] = useState<string>();
  useThumbnailEpoch();
  const key = thumbnailKey(caseDatabasePath, item, edge);
  const cachedSource = usePreparedThumbnail(key);
  useEffect(() => {
    let current = true;
    const element = container.current;
    if (!element || !caseDatabasePath) return;
    if (cachedSource) setFailedKey(previous => previous === key ? undefined : previous);
    const root = element.closest(".media-gallery-scroll");
    const priorityFor = () => {
      const bounds = element.getBoundingClientRect();
      const viewport = root?.getBoundingClientRect() ?? { top: 0, left: 0, right: window.innerWidth, bottom: window.innerHeight };
      return bounds.bottom > viewport.top && bounds.top < viewport.bottom && bounds.right > viewport.left && bounds.left < viewport.right
        ? "visible" as const : "nearby" as const;
    };
    let request: ReturnType<typeof prepareThumbnail> | undefined;
    let releaseRetention: (() => void) | undefined;
    const update = () => {
      const priority = priorityFor();
      const visible = retain || priority === "visible";
      // Protect displayed URLs from lookahead eviction. Offscreen tiles leave
      // room for the next viewport and load again when they become visible.
      if (visible) releaseRetention ??= preparedThumbnails.retain(key);
      else { releaseRetention?.(); releaseRetention = undefined; }
      if (!visible) { request?.release(); request = undefined; return; }
      if (cachedSource || request) return;
      const next = prepareThumbnail(caseDatabasePath, item, "visible", edge);
      request = next;
      void next.promise.then(source => {
        if (current && request === next && source) setFailedKey(previous => previous === key ? undefined : previous);
      }, () => {
        // Full-resolution originals can exhaust WebView RAM. Only show bounded thumbnails.
        if (current && request === next) setFailedKey(key);
      });
    };
    update();
    const visibleObserver = new IntersectionObserver(update, { root });
    visibleObserver.observe(element);
    return () => { current = false; visibleObserver.disconnect(); request?.release(); releaseRetention?.(); };
  }, [caseDatabasePath, key, item.id, item.mediaPath, cachedSource, edge, retain]);

  return (
    <div ref={container} className={cn("flex items-center justify-center overflow-hidden bg-muted", className)}>
      {failedKey === key ? (
        <ImageOff className="size-5 text-muted-foreground" aria-label={`${item.name} preview unavailable`} />
      ) : cachedSource ? (
        <img src={cachedSource} alt={item.name} className={cn("h-full w-full", fit === "contain" ? "object-contain" : "object-cover")}
          draggable={false} loading="eager" decoding="async"
          onError={() => setFailedKey(key)} />
      ) : <span className="sr-only">Loading {item.name}</span>}
    </div>
  );
}
