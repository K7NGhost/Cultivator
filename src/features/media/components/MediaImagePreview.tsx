import { ImageOff } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { preparedThumbnails, prepareThumbnail, thumbnailKey, usePreparedThumbnail, useThumbnailEpoch } from "../services/preparedThumbnails";
import type { MediaItem } from "../model/types";
import { cn } from "@/lib/utils";
import { isCsamImage } from "../model/vicClassification";
import { CSAM_IMAGE_BLUR_FILTER, CsamImageBlurIndicator } from "./CsamImageBlur";

type MediaImagePreviewProps = {
  caseDatabasePath: string | null;
  className?: string;
  fit?: "cover" | "contain";
  retain?: boolean;
  edge?: number;
  item: MediaItem;
  showFaceBoxes?: boolean;
  revealCsam?: boolean;
};

export function MediaImagePreview({ caseDatabasePath, className, fit = "cover", retain = false, edge = 256, item, showFaceBoxes = true, revealCsam = false }: MediaImagePreviewProps) {
  const blurred = isCsamImage(item) && !revealCsam;
  const container = useRef<HTMLDivElement>(null);
  const image = useRef<HTMLImageElement>(null);
  const [imageSize, setImageSize] = useState<{ key: string; width: number; height: number }>();
  const [failedKey, setFailedKey] = useState<string>();
  useThumbnailEpoch();
  const key = thumbnailKey(caseDatabasePath, item, edge);
  const cachedSource = usePreparedThumbnail(key);
  const recordImageSize = useCallback(() => {
    const element = image.current;
    if (!showFaceBoxes || !item.faceBoxes?.length || !element?.naturalWidth || !element.naturalHeight) return;
    const width = element.naturalWidth, height = element.naturalHeight;
    setImageSize(previous => previous?.key === key && previous.width === width && previous.height === height
      ? previous : { key, width, height });
  }, [key, item.faceBoxes?.length, showFaceBoxes]);
  useEffect(recordImageSize, [recordImageSize, cachedSource]);
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
    <div ref={container} data-csam-blurred={blurred || undefined} className={cn("relative flex items-center justify-center overflow-hidden bg-muted", className)}>
      {failedKey === key ? (
        <ImageOff className="size-5 text-muted-foreground" aria-label={`${item.name} preview unavailable`} />
      ) : cachedSource ? (
        <><img ref={image} src={cachedSource} alt={item.name} className={cn("h-full w-full", fit === "contain" ? "object-contain" : "object-cover")}
          style={blurred ? { filter: CSAM_IMAGE_BLUR_FILTER, transform: "scale(1.1)" } : undefined}
          draggable={false} loading="eager" decoding="async"
          onLoad={recordImageSize} onError={() => setFailedKey(key)} />
          {showFaceBoxes && !blurred && !!item.faceBoxes?.length && imageSize?.key === key && <svg className="pointer-events-none absolute inset-0 h-full w-full"
            viewBox={`0 0 ${imageSize.width} ${imageSize.height}`} preserveAspectRatio={fit === "contain" ? "xMidYMid meet" : "xMidYMid slice"} aria-hidden="true">
            {item.faceBoxes.map((face, index) => {
              const width = face.width * imageSize.width, height = face.height * imageSize.height;
              const side = Math.max(width, height);
              return <rect key={index}
                x={face.x * imageSize.width + (width - side) / 2} y={face.y * imageSize.height + (height - side) / 2}
                width={side} height={side}
                fill="none" stroke="#22d3ee" strokeWidth={2} vectorEffect="non-scaling-stroke" />;
            })}
          </svg>}
          {blurred && <CsamImageBlurIndicator />}
        </>
      ) : <span className="sr-only">Loading {item.name}</span>}
      {(item.faceCount ?? 0) > 0 && <span className="pointer-events-none absolute right-1 top-1 rounded bg-black/80 px-1 py-0.5 text-[9px] text-cyan-300"
        title="Detected faces (YuNet)" aria-label={`${item.faceCount} faces detected by YuNet`}>Faces: {item.faceCount}</span>}
    </div>
  );
}
