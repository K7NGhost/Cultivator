import { convertFileSrc } from "@tauri-apps/api/core";
import { ImageOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { prepareThumbnail, thumbnailKey, usePreparedThumbnail, useThumbnailEpoch } from "../services/preparedThumbnails";
import type { MediaItem } from "../model/types";
import { cn } from "@/lib/utils";

type MediaImagePreviewProps = {
  caseDatabasePath: string | null;
  className?: string;
  item: MediaItem;
};

export function MediaImagePreview({ caseDatabasePath, className, item }: MediaImagePreviewProps) {
  const container = useRef<HTMLDivElement>(null);
  const [preview, setPreview] = useState<{ key: string; source?: string; failed?: boolean }>();
  useThumbnailEpoch();
  const key = thumbnailKey(caseDatabasePath, item);
  const cachedSource = usePreparedThumbnail(key);

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
    // Every mounted overscan tile starts loading now, even when it is several
    // screens outside the clipped viewport. The range prefetcher goes further.
    const request = prepareThumbnail(caseDatabasePath, item, priorityFor());
    request.promise.then((source) => {
      if (current && source) setPreview({ key, source });
    }).catch(() => {
      // Keep browser-supported images viewable if the decoder is unavailable.
      if (current) setPreview({ key, source: convertFileSrc(item.mediaPath) });
    });
    const visibleObserver = new IntersectionObserver(() => request.setPriority(priorityFor()), { root });
    visibleObserver.observe(element);
    return () => { current = false; visibleObserver.disconnect(); request.release(); };
  }, [caseDatabasePath, key, item.id, item.mediaPath]);

  const state: { source?: string; failed?: boolean } | undefined = preview?.key === key ? preview : cachedSource ? { source: cachedSource } : undefined;
  return (
    <div ref={container} className={cn("flex items-center justify-center overflow-hidden bg-muted", className)}>
      {state?.failed ? (
        <ImageOff className="size-5 text-muted-foreground" aria-label={`${item.name} preview unavailable`} />
      ) : state?.source ? (
        <img src={state.source} alt={item.name} className="h-full w-full object-cover"
          draggable={false} loading="eager" decoding="async"
          onError={() => setPreview({ key, failed: true })} />
      ) : <span className="sr-only">Loading {item.name}</span>}
    </div>
  );
}
