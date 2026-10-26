import { useEffect, useMemo, useRef, useState } from "react";
import {
  Layers,
  LocateFixed,
  MapPin,
  Pause,
  Play,
  RotateCcw,
  Satellite,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  MapCanvas,
  type Basemap,
  type MapHandle,
  type MapStatus,
} from "./MapCanvas";
import { hasGps } from "../lib/mapData";
import { elapsedLabel, finiteLabel, pointAtTime } from "../lib/telemetry";
import type { FlightRecord } from "../types";

export function FlightMap({
  records,
  selected,
  onSelect,
  actualPointCount,
}: {
  records: FlightRecord[];
  selected: FlightRecord;
  onSelect: (record: FlightRecord) => void;
  actualPointCount: number;
}) {
  const map = useRef<MapHandle>(null);
  const [basemap, setBasemap] = useState<Basemap>("streets");
  const [showPath, setShowPath] = useState(true);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(4);
  const [status, setStatus] = useState<MapStatus>({ phase: "loading" });
  const [version, setVersion] = useState(0);
  const points = useMemo(() => records.filter(hasGps), [records]);
  const index = pointAtTime(points, selected.elapsedSeconds ?? 0);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  useEffect(() => {
    if (!playing || points.length < 2) return;
    const start = performance.now();
    const origin = selectedRef.current.elapsedSeconds ?? 0;
    const timer = window.setInterval(() => {
      const elapsed = origin + ((performance.now() - start) / 1000) * speed;
      const next = points[pointAtTime(points, elapsed)];
      if (next) onSelect(next);
      if (elapsed >= (points[points.length - 1]?.elapsedSeconds ?? 0))
        setPlaying(false);
    }, 100);
    return () => window.clearInterval(timer);
  }, [playing, points, speed, onSelect]);
  return (
    <section
      className="flex min-h-[440px] flex-col overflow-hidden rounded-xl border bg-card shadow-sm"
      aria-label="Flight path and playback"
    >
      <div className="flex items-center justify-between border-b px-4 py-3">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <MapPin className="size-4 text-teal-500" /> Flight path{" "}
          <span className="ml-2 text-xs font-normal text-muted-foreground">
            {actualPointCount.toLocaleString()} GPS fixes
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Satellite className="size-3.5 text-muted-foreground" />
          <select
            aria-label="Map basemap"
            className="rounded-md border bg-background px-2 py-1 text-xs"
            value={basemap}
            onChange={(e) => setBasemap(e.target.value as Basemap)}
          >
            <option value="streets">Streets</option>
            <option value="satellite">Satellite</option>
          </select>
        </div>
      </div>
      <div className="relative min-h-[320px] flex-1 bg-muted">
        <MapCanvas
          key={version}
          ref={map}
          records={points}
          selected={selected}
          basemap={basemap}
          showPath={showPath}
          onStatus={setStatus}
        />
        <div className="pointer-events-none absolute left-3 top-3 rounded-lg border bg-background/95 px-3 py-2 text-xs shadow-sm backdrop-blur">
          <div className="flex items-center gap-2 font-medium">
            <span className="size-1.5 rounded-full bg-teal-500" />
            {points[0]?.coordinateSource?.toUpperCase() ?? "GPS"} track
          </div>
          <p className="mt-1 text-[10px] text-muted-foreground">
            {points.length < actualPointCount
              ? `${points.length.toLocaleString()} fixes rendered · full track retained in CSV`
              : "Coordinates decoded from DAT"}
          </p>
        </div>
        <div className="absolute right-3 top-3 flex flex-col gap-1.5">
          <Button
            size="icon"
            variant="outline"
            className="size-8 bg-background"
            aria-label="Fit flight path"
            onClick={() => map.current?.fitFlight()}
          >
            <LocateFixed className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="outline"
            className="size-8 bg-background"
            aria-label="Toggle flight path"
            aria-pressed={showPath}
            onClick={() => setShowPath((v) => !v)}
          >
            <Layers className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="outline"
            className="size-8 bg-background"
            aria-label="Reset north"
            onClick={() => map.current?.resetNorth()}
          >
            <RotateCcw className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="outline"
            className="size-8 bg-background"
            aria-label="Zoom in"
            onClick={() => map.current?.zoomIn()}
          >
            +
          </Button>
          <Button
            size="icon"
            variant="outline"
            className="size-8 bg-background"
            aria-label="Zoom out"
            onClick={() => map.current?.zoomOut()}
          >
            −
          </Button>
        </div>
        {status.phase === "loading" && (
          <div
            role="status"
            className="absolute bottom-3 left-3 rounded-md bg-background/95 px-3 py-2 text-xs shadow-sm"
          >
            Loading map…
          </div>
        )}
        {(status.offline || status.phase === "failed") && (
          <div
            role="status"
            className="absolute bottom-3 left-3 right-3 rounded-lg border bg-background/95 p-3 text-xs shadow-sm"
          >
            {status.message}
            <button
              className="ml-2 underline"
              onClick={() => {
                setVersion((v) => v + 1);
                setStatus({ phase: "loading" });
              }}
            >
              Retry map
            </button>
          </div>
        )}
        {!points.length && status.phase === "ready" && (
          <div className="absolute inset-0 grid place-items-center bg-background/70 text-sm text-muted-foreground">
            No valid GPS fixes. Telemetry is available below.
          </div>
        )}
      </div>
      <div className="space-y-3 border-t px-4 py-3">
        <div className="flex items-center justify-between text-[10px] uppercase tracking-wider text-muted-foreground">
          <span>Flight playback</span>
          <span className="font-mono normal-case">{selected.time} elapsed</span>
        </div>
        <div className="flex items-center gap-3">
          <Button
            size="icon"
            className="size-8 shrink-0 bg-teal-600 text-white hover:bg-teal-700"
            aria-label={playing ? "Pause playback" : "Play flight"}
            disabled={points.length < 2}
            onClick={() => {
              if (!playing && index >= points.length - 1) onSelect(points[0]);
              setPlaying((v) => !v);
            }}
          >
            {playing ? (
              <Pause className="size-3.5" />
            ) : (
              <Play className="size-3.5" />
            )}
          </Button>
          <input
            aria-label="Flight timeline"
            type="range"
            min={0}
            max={Math.max(0, points.length - 1)}
            value={index}
            disabled={!points.length}
            className="min-w-0 flex-1 accent-teal-600"
            onChange={(e) => {
              setPlaying(false);
              const point = points[Number(e.target.value)];
              if (point) onSelect(point);
            }}
          />
          <select
            aria-label="Playback speed"
            className="rounded border bg-background p-1 text-xs"
            value={speed}
            onChange={(e) => {
              setPlaying(false);
              setSpeed(Number(e.target.value));
            }}
          >
            <option value={1}>1×</option>
            <option value={4}>4×</option>
            <option value={10}>10×</option>
          </select>
          <span className="font-mono text-[10px] text-muted-foreground">
            {elapsedLabel(points[points.length - 1]?.elapsedSeconds ?? 0)}
          </span>
        </div>
        <div className="grid grid-cols-3 border-t pt-3 text-xs">
          <div>
            <p className="text-[10px] text-muted-foreground">Latitude</p>
            <p className="mt-1 font-mono">
              {finiteLabel(selected.latitude, 6)}°
            </p>
          </div>
          <div>
            <p className="text-[10px] text-muted-foreground">Longitude</p>
            <p className="mt-1 font-mono">
              {finiteLabel(selected.longitude, 6)}°
            </p>
          </div>
          <div>
            <p className="text-[10px] text-muted-foreground">
              Altitude ·{" "}
              {selected.altitudeReference?.replace(/_/g, " ") ??
                "unknown reference"}
            </p>
            <p className="mt-1 font-mono">{finiteLabel(selected.altitude)} m</p>
          </div>
        </div>
      </div>
    </section>
  );
}
