import { useEffect, useMemo, useRef, useState, type PointerEvent, type WheelEvent } from "react";
import { useLocation } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Copy, Crosshair, ExternalLink, MapPin, RefreshCw } from "lucide-react";
import { toast } from "react-toastify";
import land from "world-atlas/land-50m.json";

import { useReveal } from "@/app/reveal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { listArtifacts } from "@/features/artifacts/artifactRepository";
import type { StoredArtifactRecord } from "@/features/artifacts/types";
import { useCases } from "@/features/cases/case-provider";
import { mediaLocation, project, topologyPath, validPoint } from "@/features/map/geo";
import { listMediaGallery } from "@/features/media/mediaRepository";
import type { MediaItem } from "@/features/media/types";
import { useElementSize } from "@/lib/useElementSize";
import { cn } from "@/lib/utils";

type MapPoint = {
  id: string;
  lat: number;
  lon: number;
  label: string;
  detail: string;
  kind: "photo" | "video" | "artifact";
  item?: MediaItem;
  artifactId?: string;
};

type View = { cx: number; cy: number; unitsPerPixel: number };

const WORLD: View = { cx: 180, cy: 90, unitsPerPixel: 0.5 };
// Rendered once: the world's land outline (Natural Earth 1:50m), bundled with
// the app so no map service ever learns which places an examiner looks at.
const LAND_PATH = topologyPath(land as unknown as Parameters<typeof topologyPath>[0], "land");

export function MapPage() {
  const { activeCase } = useCases();
  const location = useLocation();
  const isActive = location.pathname === "/map";
  const reveal = useReveal();
  const [points, setPoints] = useState<MapPoint[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [view, setView] = useState<View>(WORLD);
  const [containerRef, size] = useElementSize<HTMLDivElement>();
  const dragRef = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (!activeCase || !isActive) {
      if (!activeCase) {
        setPoints([]);
      }
      return;
    }

    let isCurrent = true;
    setIsLoading(true);
    Promise.all([listMediaGallery(activeCase.databasePath, null), listArtifacts(activeCase.databasePath)])
      .then(([gallery, artifacts]) => {
        if (!isCurrent) {
          return;
        }
        const found: MapPoint[] = [];
        for (const item of [...gallery.photos, ...gallery.videos]) {
          const point = mediaLocation(item);
          if (point) {
            found.push({
              id: `media:${item.path}`,
              ...point,
              label: item.name,
              detail: item.caption ?? "",
              kind: item.mediaType === "video" ? "video" : "photo",
              item,
            });
          }
        }
        for (const record of artifacts) {
          const payload = readPayload(record.payload);
          const point = validPoint(
            numberField(payload.latitude ?? payload.lat),
            numberField(payload.longitude ?? payload.lon ?? payload.lng),
          );
          if (point) {
            found.push({
              id: `artifact:${record.id}`,
              ...point,
              label: String(record.label ?? payload.kind ?? "Location"),
              detail: String(payload.kind ?? record.resultKind ?? ""),
              kind: "artifact",
              artifactId: record.id,
            });
          }
        }
        setPoints(found);
      })
      .catch((error) => toast.error(error instanceof Error ? error.message : String(error)))
      .finally(() => {
        if (isCurrent) {
          setIsLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase, isActive, refreshKey]);

  const fit = () => setView(fitView(points, size));

  // Frame the points whenever a new set arrives.
  useEffect(() => {
    if (size.width > 0) {
      setView(fitView(points, size));
    }
  }, [points, size.width > 0]);

  const selected = points.find((point) => point.id === selectedId) ?? null;
  const viewBox = `${view.cx - (size.width * view.unitsPerPixel) / 2} ${view.cy - (size.height * view.unitsPerPixel) / 2} ${size.width * view.unitsPerPixel} ${size.height * view.unitsPerPixel}`;
  const graticule = useMemo(() => {
    const lines: string[] = [];
    for (let lon = 30; lon < 360; lon += 30) lines.push(`M${lon},0V180`);
    for (let lat = 30; lat < 180; lat += 30) lines.push(`M0,${lat}H360`);
    return lines.join("");
  }, []);

  function zoom(event: WheelEvent<SVGSVGElement>) {
    const rect = event.currentTarget.getBoundingClientRect();
    const px = event.clientX - rect.left - rect.width / 2;
    const py = event.clientY - rect.top - rect.height / 2;
    setView((current) => {
      const unitsPerPixel = clamp(current.unitsPerPixel * Math.exp(event.deltaY * 0.0015), 0.00002, 1);
      // Keep the spot under the pointer in place.
      return {
        cx: current.cx + px * (current.unitsPerPixel - unitsPerPixel),
        cy: current.cy + py * (current.unitsPerPixel - unitsPerPixel),
        unitsPerPixel,
      };
    });
  }

  function drag(event: PointerEvent<SVGSVGElement>) {
    const start = dragRef.current;
    if (!start) {
      return;
    }
    const [dx, dy] = [event.clientX - start.x, event.clientY - start.y];
    dragRef.current = { x: event.clientX, y: event.clientY };
    setView((current) => ({
      ...current,
      cx: current.cx - dx * current.unitsPerPixel,
      cy: current.cy - dy * current.unitsPerPixel,
    }));
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <MapPin className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <h1 className="text-sm font-semibold">Map</h1>
        <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
          {points.length.toLocaleString()} located items
        </Badge>
        <span className="truncate text-[11px] text-muted-foreground">
          Photo GPS and location artifacts. The map is built in; nothing is sent online.
        </span>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="ml-auto h-7 rounded-sm px-2 text-xs"
          disabled={points.length === 0}
          onClick={fit}
        >
          <Crosshair className="size-3.5" />
          Fit
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          disabled={!activeCase || isLoading}
          onClick={() => setRefreshKey((key) => key + 1)}
        >
          <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
          Refresh
        </Button>
      </section>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_18rem]">
        <div ref={containerRef} className="relative min-h-0 overflow-hidden bg-sky-50 dark:bg-slate-950">
          {size.width > 0 && (
            <svg
              className="size-full cursor-grab touch-none select-none active:cursor-grabbing"
              viewBox={viewBox}
              onWheel={zoom}
              onPointerDown={(event) => {
                event.currentTarget.setPointerCapture(event.pointerId);
                dragRef.current = { x: event.clientX, y: event.clientY };
              }}
              onPointerMove={drag}
              onPointerUp={() => {
                dragRef.current = null;
              }}
              role="img"
              aria-label="Map of located items"
            >
              <path d={graticule} className="stroke-slate-300 dark:stroke-slate-800" fill="none" strokeWidth={1} vectorEffect="non-scaling-stroke" />
              <path d={LAND_PATH} className="fill-stone-200 stroke-stone-400 dark:fill-slate-800 dark:stroke-slate-600" strokeWidth={0.6} vectorEffect="non-scaling-stroke" />
              {points.map((point) => {
                const { x, y } = project(point);
                const isSelected = point.id === selectedId;
                return (
                  <circle
                    key={point.id}
                    cx={x}
                    cy={y}
                    r={(isSelected ? 7 : 5) * view.unitsPerPixel}
                    className={cn(
                      "cursor-pointer stroke-white",
                      point.kind === "artifact" ? "fill-emerald-600" : "fill-red-600",
                      isSelected && "fill-amber-500",
                    )}
                    strokeWidth={1.5}
                    vectorEffect="non-scaling-stroke"
                    onPointerDown={(event) => event.stopPropagation()}
                    onClick={() => setSelectedId(point.id)}
                  >
                    <title>{`${point.label} (${point.lat.toFixed(5)}, ${point.lon.toFixed(5)})`}</title>
                  </circle>
                );
              })}
            </svg>
          )}
          {!isLoading && points.length === 0 && (
            <div className="pointer-events-none absolute inset-x-0 top-3 mx-auto w-fit rounded-sm border bg-background px-2 py-1 text-xs text-muted-foreground">
              {activeCase
                ? "No located items yet. Run Image Metadata to read photo GPS."
                : "Open a case to see where its photos were taken."}
            </div>
          )}
        </div>

        <aside className="flex min-h-0 flex-col border-l">
          {selected ? (
            <div className="grid gap-2 border-b p-2 text-xs">
              {selected.item && selected.kind === "photo" && (
                <img
                  src={convertFileSrc(selected.item.thumbnailPath || selected.item.mediaPath)}
                  alt={selected.label}
                  className="max-h-48 w-full rounded-sm object-contain"
                />
              )}
              <div className="font-medium break-words">{selected.label}</div>
              {selected.detail && <div className="text-muted-foreground">{selected.detail}</div>}
              <div className="font-mono text-[11px]">
                {selected.lat.toFixed(6)}, {selected.lon.toFixed(6)}
              </div>
              <div className="flex gap-1">
                <Button
                  type="button"
                  variant="outline"
                  size="xs"
                  className="h-6 rounded-sm px-1.5 text-[11px]"
                  onClick={() => {
                    void navigator.clipboard.writeText(`${selected.lat.toFixed(6)}, ${selected.lon.toFixed(6)}`);
                    toast.success("Coordinates copied.");
                  }}
                >
                  <Copy className="size-3" />
                  Copy coordinates
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="xs"
                  className="h-6 rounded-sm px-1.5 text-[11px]"
                  onClick={() =>
                    selected.item
                      ? reveal({ kind: "media", path: selected.item.path, mediaType: selected.item.mediaType })
                      : selected.artifactId && reveal({ kind: "artifact", artifactId: selected.artifactId })
                  }
                >
                  <ExternalLink className="size-3" />
                  Open
                </Button>
              </div>
            </div>
          ) : (
            <p className="border-b p-2 text-xs text-muted-foreground">
              Click a point to see what was there. Scroll to zoom, drag to move.
            </p>
          )}
          <ul className="min-h-0 flex-1 overflow-auto text-xs">
            {points.map((point) => (
              <li key={point.id}>
                <button
                  type="button"
                  className={cn(
                    "flex w-full items-center gap-1.5 px-2 py-1 text-left hover:bg-accent",
                    point.id === selectedId && "bg-accent",
                  )}
                  onClick={() => {
                    setSelectedId(point.id);
                    const { x, y } = project(point);
                    setView((current) => ({ ...current, cx: x, cy: y }));
                  }}
                >
                  <MapPin
                    className={cn("size-3 shrink-0", point.kind === "artifact" ? "text-emerald-600" : "text-red-600")}
                    aria-hidden="true"
                  />
                  <span className="truncate">{point.label}</span>
                  <span className="ml-auto shrink-0 font-mono text-[10px] text-muted-foreground">
                    {point.lat.toFixed(3)}, {point.lon.toFixed(3)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </aside>
      </div>
    </div>
  );
}

/** Frames every point with a margin, or shows the whole world. */
function fitView(points: MapPoint[], size: { width: number; height: number }): View {
  if (points.length === 0 || size.width === 0 || size.height === 0) {
    return { cx: 180, cy: 90, unitsPerPixel: Math.max(360 / (size.width || 720), 180 / (size.height || 360)) };
  }
  const xs = points.map((point) => project(point).x);
  const ys = points.map((point) => project(point).y);
  const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  // At least ~0.02 degrees across, so a single point shows its neighbourhood.
  const spanX = Math.max(maxX - minX, 0.02);
  const spanY = Math.max(maxY - minY, 0.02);
  return {
    cx: (minX + maxX) / 2,
    cy: (minY + maxY) / 2,
    unitsPerPixel: Math.max(spanX / size.width, spanY / size.height) * 1.3,
  };
}

function readPayload(payload: StoredArtifactRecord["payload"]): Record<string, unknown> {
  if (payload && typeof payload === "object") {
    return payload as Record<string, unknown>;
  }
  if (typeof payload === "string") {
    try {
      const parsed: unknown = JSON.parse(payload);
      return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : {};
    } catch {
      return {};
    }
  }
  return {};
}

function numberField(value: unknown): number | null {
  const number = typeof value === "number" ? value : typeof value === "string" ? Number(value) : NaN;
  return Number.isFinite(number) ? number : null;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}
