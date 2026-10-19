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
  item: MediaItem;
};

export function MediaImagePreview({ caseDatabasePath, className, fit = "cover", retain = false, item }: MediaImagePreviewProps) {
  const container = useRef<HTMLDivElement>(null);
  const [failedKey, setFailedKey] = useState<string>();
  useThumbnailEpoch();
  const key = thumbnailKey(caseDatabasePath, item);
  const cachedSource = usePreparedThumbnail(key);
  useEffect(() => retain ? preparedThumbnails.retain(key) : undefined, [key, retain]);

  useEffect(() => {
    let current = true;
    const element = container.current;
    if (!element || !caseDatabasePath) return;
    const root = element.closest(".media-gallery-scroll");
    const priorityFor = () => {
      const bounds = element.getBoundingClientRect();
      const viewport = root?.getBoundingClientRect() ?? { top: 0, left: 0, right: window.innerWidth, bottom: window.innerHeight };
      return bounds.bottom > viewport.top && bounds.top < viewport.bottom && bounds.right > viewport.left && bounds.left < viewport.right
        ? "visible" as const : "nearby" as const;
    };
    // Visible tiles take priority over overscan in the bounded loading queue.
    const request = prepareThumbnail(caseDatabasePath, item, priorityFor());
    request.promise.catch(() => {
      // Full-resolution originals can exhaust WebView RAM. Only show bounded thumbnails.
      if (current) setFailedKey(key);
    });
    const visibleObserver = new IntersectionObserver(() => request.setPriority(priorityFor()), { root });
    visibleObserver.observe(element);
    return () => { current = false; visibleObserver.disconnect(); request.release(); };
  }, [caseDatabasePath, key, item.id, item.mediaPath]);

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
