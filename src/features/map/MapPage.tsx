import { useEffect, useMemo, useRef, useState, type PointerEvent, type WheelEvent } from "react";
import { useLocation } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { geoEquirectangular, geoGraticule10, geoPath } from "d3-geo";
import { Copy, Crosshair, ExternalLink, MapPin, Minus, Plus, RefreshCw, X } from "lucide-react";
import { toast } from "react-toastify";
import countries from "world-atlas/countries-50m.json";
import land from "world-atlas/land-50m.json";

import { useReveal } from "@/app/reveal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { listArtifacts } from "@/features/artifacts/artifactRepository";
import type { StoredArtifactRecord } from "@/features/artifacts/types";
import { useCases } from "@/features/cases/case-provider";
import {
  kilometresPerDegree,
  mediaLocation,
  project,
  roundLength,
  topologyFeatures,
  validPoint,
} from "@/features/map/geo";
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

type Cluster = { key: string; x: number; y: number; members: MapPoint[] };

type View = { cx: number; cy: number; unitsPerPixel: number };

// d3 draws in the same numbers as `project`: x = lon + 180, y = 90 - lat. It
// also cuts shapes at the date line and around the poles, which a plain
// polygon drawing gets wrong (Russia's tip smeared a band across the map).
const projection = geoEquirectangular().scale(180 / Math.PI).translate([180, 90]).precision(0);
const drawPath = geoPath(projection);
// Rendered once. The world outline (Natural Earth 1:50m) is bundled with the
// app, so no map service ever learns which places an examiner looks at.
const LAND_PATH = drawPath(topologyFeatures(land as never, "land")) ?? "";
const BORDER_PATH = drawPath(topologyFeatures(countries as never, "countries")) ?? "";
const GRATICULE_PATH = drawPath(geoGraticule10()) ?? "";
const CLUSTER_CELL_PIXELS = 34;

export function MapPage() {
  const { activeCase } = useCases();
  const location = useLocation();
  const isActive = location.pathname === "/map";
  const reveal = useReveal();
  const [points, setPoints] = useState<MapPoint[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [group, setGroup] = useState<MapPoint[] | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [view, setView] = useState<View>({ cx: 180, cy: 90, unitsPerPixel: 0.5 });
  const [cursor, setCursor] = useState<{ lat: number; lon: number } | null>(null);
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
        if (isCurrent) {
          setPoints(locatedPoints([...gallery.photos, ...gallery.videos], artifacts));
          setGroup(null);
        }
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

  // Frame the points whenever a new set arrives.
  useEffect(() => {
    if (size.width > 0) {
      setView(fitView(points, size));
    }
  }, [points, size.width > 0]);

  // Points close together on screen share one numbered marker.
  const clusters = useMemo(() => {
    const cell = CLUSTER_CELL_PIXELS * view.unitsPerPixel;
    const byCell = new Map<string, Cluster>();
    for (const point of points) {
      const { x, y } = project(point);
      const key = `${Math.floor(x / cell)}:${Math.floor(y / cell)}`;
      const cluster = byCell.get(key) ?? { key, x: 0, y: 0, members: [] };
      cluster.members.push(point);
      cluster.x += x;
      cluster.y += y;
      byCell.set(key, cluster);
    }
    return [...byCell.values()].map((cluster) => ({
      ...cluster,
      x: cluster.x / cluster.members.length,
      y: cluster.y / cluster.members.length,
    }));
  }, [points, view.unitsPerPixel]);

  const selected = points.find((point) => point.id === selectedId) ?? null;
  const u = view.unitsPerPixel;
  const viewBox = `${view.cx - (size.width * u) / 2} ${view.cy - (size.height * u) / 2} ${size.width * u} ${size.height * u}`;
  const listed = group ?? points;

  // A scale bar about 100 px long, measured at the middle of the view.
  const kmPerPixel = u * kilometresPerDegree(Math.min(85, Math.abs(90 - view.cy)));
  const scaleKm = roundLength(kmPerPixel * 110);
  const scalePixels = scaleKm / kmPerPixel;

  function zoomBy(factor: number, px = 0, py = 0) {
    setView((current) => {
      const unitsPerPixel = clamp(current.unitsPerPixel * factor, 0.00002, 1);
      // Keep the spot under the pointer (or the centre) in place.
      return {
        cx: current.cx + px * (current.unitsPerPixel - unitsPerPixel),
        cy: current.cy + py * (current.unitsPerPixel - unitsPerPixel),
        unitsPerPixel,
      };
    });
  }

  function onWheel(event: WheelEvent<SVGSVGElement>) {
    const rect = event.currentTarget.getBoundingClientRect();
    zoomBy(
      Math.exp(event.deltaY * 0.0015),
      event.clientX - rect.left - rect.width / 2,
      event.clientY - rect.top - rect.height / 2,
    );
  }

  function onPointerMove(event: PointerEvent<SVGSVGElement>) {
    const rect = event.currentTarget.getBoundingClientRect();
    const x = view.cx + (event.clientX - rect.left - rect.width / 2) * u;
    const y = view.cy + (event.clientY - rect.top - rect.height / 2) * u;
    setCursor({ lon: x - 180, lat: 90 - y });

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

  function openCluster(cluster: Cluster) {
    if (cluster.members.length === 1) {
      setSelectedId(cluster.members[0].id);
      return;
    }
    const xs = cluster.members.map((point) => project(point).x);
    const ys = cluster.members.map((point) => project(point).y);
    const spread = Math.max(Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys));
    // Items at (almost) the same spot can't be pulled apart by zooming.
    if (spread < 0.0005) {
      setGroup(cluster.members);
      setSelectedId(cluster.members[0].id);
    } else {
      setView(fitView(cluster.members, size));
    }
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
          disabled={!activeCase || isLoading}
          onClick={() => setRefreshKey((key) => key + 1)}
        >
          <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
          Refresh
        </Button>
      </section>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_18rem]">
        <div
          ref={containerRef}
          className="relative min-h-0 overflow-hidden bg-gradient-to-b from-sky-100 via-sky-50 to-sky-100 dark:from-slate-950 dark:via-slate-900 dark:to-slate-950"
        >
          {size.width > 0 && (
            <svg
              className="size-full cursor-grab touch-none select-none active:cursor-grabbing"
              viewBox={viewBox}
              onWheel={onWheel}
              onPointerDown={(event) => {
                event.currentTarget.setPointerCapture(event.pointerId);
                dragRef.current = { x: event.clientX, y: event.clientY };
              }}
              onPointerMove={onPointerMove}
              onPointerUp={() => {
                dragRef.current = null;
              }}
              onPointerLeave={() => setCursor(null)}
              role="img"
              aria-label="Map of located items"
            >
              <path d={GRATICULE_PATH} fill="none" className="stroke-sky-200 dark:stroke-slate-800" strokeWidth={0.8} vectorEffect="non-scaling-stroke" />
              <path d={LAND_PATH} className="fill-[#f4efe4] stroke-[#b8ad97] dark:fill-slate-800 dark:stroke-slate-600" strokeWidth={0.9} vectorEffect="non-scaling-stroke" />
              <path d={BORDER_PATH} fill="none" className="stroke-[#d3c9b5] dark:stroke-slate-700" strokeWidth={0.6} vectorEffect="non-scaling-stroke" />
              {clusters.map((cluster) => {
                const single = cluster.members.length === 1 ? cluster.members[0] : null;
                const isSelected = cluster.members.some((point) => point.id === selectedId);
                const radius = single ? 6 : Math.min(22, 9 + 3 * Math.log2(cluster.members.length));
                const color = single?.kind === "artifact" ? "fill-emerald-600" : "fill-rose-600";
                return (
                  <g
                    key={cluster.key}
                    className="cursor-pointer"
                    onPointerDown={(event) => event.stopPropagation()}
                    onClick={() => openCluster(cluster)}
                  >
                    {isSelected && (
                      <circle cx={cluster.x} cy={cluster.y} r={(radius + 6) * u} className="fill-amber-400/40" />
                    )}
                    <circle
                      cx={cluster.x}
                      cy={cluster.y}
                      r={radius * u}
                      className={cn(color, "stroke-white drop-shadow")}
                      strokeWidth={2}
                      vectorEffect="non-scaling-stroke"
                    />
                    {!single && (
                      <text
                        x={cluster.x}
                        y={cluster.y}
                        dy="0.35em"
                        textAnchor="middle"
                        fontSize={11 * u}
                        fontWeight={600}
                        className="pointer-events-none fill-white"
                      >
                        {cluster.members.length}
                      </text>
                    )}
                    <title>
                      {single
                        ? `${single.label} (${single.lat.toFixed(5)}, ${single.lon.toFixed(5)})`
                        : `${cluster.members.length} items - click to zoom in or list them`}
                    </title>
                  </g>
                );
              })}
            </svg>
          )}

          <div className="absolute top-2 right-2 flex flex-col overflow-hidden rounded-sm border bg-background/90 shadow-sm">
            <button type="button" className="p-1.5 hover:bg-accent" aria-label="Zoom in" onClick={() => zoomBy(0.5)}>
              <Plus className="size-3.5" />
            </button>
            <button type="button" className="border-t p-1.5 hover:bg-accent" aria-label="Zoom out" onClick={() => zoomBy(2)}>
              <Minus className="size-3.5" />
            </button>
            <button
              type="button"
              className="border-t p-1.5 hover:bg-accent disabled:opacity-40"
              aria-label="Fit all points"
              disabled={points.length === 0}
              onClick={() => setView(fitView(points, size))}
            >
              <Crosshair className="size-3.5" />
            </button>
          </div>

          <div className="pointer-events-none absolute bottom-2 left-2 grid gap-1 rounded-sm border bg-background/90 px-2 py-1 text-[10px] shadow-sm">
            <div className="flex items-end gap-1.5">
              <div className="h-1.5 border-x border-b border-foreground" style={{ width: scalePixels }} />
              <span>{scaleKm >= 1 ? `${scaleKm.toLocaleString()} km` : `${Math.round(scaleKm * 1000)} m`}</span>
            </div>
            <div className="font-mono text-muted-foreground">
              {cursor ? `${cursor.lat.toFixed(5)}, ${cursor.lon.toFixed(5)}` : " "}
            </div>
          </div>

          <div className="pointer-events-none absolute bottom-2 right-2 flex gap-2 rounded-sm border bg-background/90 px-2 py-1 text-[10px] shadow-sm">
            <span className="flex items-center gap-1">
              <span className="size-2 rounded-full bg-rose-600" /> Photo / video
            </span>
            <span className="flex items-center gap-1">
              <span className="size-2 rounded-full bg-emerald-600" /> Location artifact
            </span>
          </div>

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
                  className="max-h-48 w-full rounded-sm bg-muted object-contain"
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
              Click a marker to see what was there. Numbers are several items close together: click to
              zoom in. Scroll to zoom, drag to move.
            </p>
          )}
          {group && (
            <div className="flex items-center gap-1 border-b px-2 py-1 text-[11px]">
              <span className="font-medium">{group.length.toLocaleString()} items at this spot</span>
              <button
                type="button"
                className="ml-auto flex items-center gap-0.5 text-muted-foreground hover:text-foreground"
                onClick={() => setGroup(null)}
              >
                <X className="size-3" /> Show all
              </button>
            </div>
          )}
          <ul className="min-h-0 flex-1 overflow-auto text-xs">
            {listed.map((point) => (
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
                    className={cn("size-3 shrink-0", point.kind === "artifact" ? "text-emerald-600" : "text-rose-600")}
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

/** Photos and videos with EXIF GPS, and plugin artifacts that carry coordinates. */
function locatedPoints(media: MediaItem[], artifacts: StoredArtifactRecord[]): MapPoint[] {
  const found: MapPoint[] = [];
  for (const item of media) {
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
        label: String(record.label || payload.kind || "Location"),
        detail: String(payload.kind ?? record.resultKind ?? ""),
        kind: "artifact",
        artifactId: record.id,
      });
    }
  }
  return found;
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
