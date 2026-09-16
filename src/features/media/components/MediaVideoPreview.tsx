import { Film, Play } from "lucide-react";
import { useEffect, useState } from "react";

import type { MediaItem } from "@/features/media/types";
import { getVideoThumbnail } from "@/features/media/videoThumbnails";
import { cn } from "@/lib/utils";

export function MediaVideoPreview({ item, className }: {
  item: MediaItem;
  className?: string;
}) {
  const path = item.mediaPath || item.path;
  const key = JSON.stringify([path, item.size, item.modifiedMs]);
  const [preview, setPreview] = useState<{ key: string; source?: string; failed?: boolean }>();

  useEffect(() => {
    const controller = new AbortController();
    getVideoThumbnail(key, path, controller.signal).then(
      (source) => {
        if (!controller.signal.aborted) setPreview({ key, source });
      },
      () => {
        if (!controller.signal.aborted) setPreview({ key, failed: true });
      },
    );
    return () => controller.abort();
  }, [key, path]);

  const current = preview?.key === key ? preview : undefined;
  return (
    <div className={cn("relative flex items-center justify-center bg-muted", className)}>
      {current?.source ? (
        <img src={current.source} alt={`Four frames from ${item.name}`}
          className="h-full w-full object-contain" draggable={false} />
      ) : (
        <div className="flex flex-col items-center gap-1 text-muted-foreground"
          role="status">
          <Film className="size-5" aria-hidden="true" />
          <span className="text-[10px]">
            {current?.failed ? "Preview unavailable" : "Loading frames…"}
          </span>
        </div>
      )}
      <span className="absolute bottom-1 right-1 rounded-sm bg-black/70 p-1 text-white"
        aria-hidden="true">
        <Play className="size-3" />
      </span>
    </div>
  );
}
