import { useEffect, useMemo, useRef, useState } from "react";
import type { GeoJSONSource, Map as LibreMap, Marker } from "maplibre-gl";
import workerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";
import "maplibre-gl/dist/maplibre-gl.css";
import { LocateFixed, MapPin, RefreshCw, ChevronLeft, ChevronRight, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { OpenLocationInBrowserButton } from "@/components/OpenLocationInBrowserButton";
import { basemapStyle, localStyle, type Basemap } from "@/lib/maps/mapStyles";
import { hasBasemapStyle, installBasemapImageResolver } from "@/lib/maps/mapImages";
import { getArtifactLocations, locationBounds } from "../utils/artifactLocations";
import { formatArtifactTableCell } from "../utils/artifactTableFormatting";
import { getCustomTableRowSources, type CustomTableView } from "../utils/customTableGrouping";

const SOURCE = "artifact-locations";
const PINS = "artifact-location-pins";
const CLUSTERS = "artifact-location-clusters";

function pinImage() {
  const canvas = document.createElement("canvas");
  canvas.width = 32; canvas.height = 40;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Cannot draw map pins.");
  context.beginPath();
  context.moveTo(16, 39); context.bezierCurveTo(13, 31, 2, 23, 2, 15);
  context.arc(16, 15, 14, Math.PI, 0); context.bezierCurveTo(30, 23, 19, 31, 16, 39);
  context.fillStyle = "#0d9488"; context.fill();
  context.strokeStyle = "#ffffff"; context.lineWidth = 2; context.stroke();
  context.beginPath(); context.arc(16, 15, 5, 0, Math.PI * 2);
  context.fillStyle = "#ffffff"; context.fill();
  return context.getImageData(0, 0, 32, 40);
}

export function ArtifactLocationPreview({ table, onSelectArtifactId, selectedArtifactId }: {
  table: CustomTableView;
  onSelectArtifactId?: (id: string) => void;
  selectedArtifactId?: string;
}) {
  const { points, pairs, invalidRows } = useMemo(() => getArtifactLocations(table), [table]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [detailsOpen, setDetailsOpen] = useState(true);
  const [basemap, setBasemap] = useState<Basemap>("streets");
  const [retry, setRetry] = useState(0);
  const [readyVersion, setReadyVersion] = useState(0);
  const [status, setStatus] = useState("Loading map…");
  const container = useRef<HTMLDivElement>(null);
  const mapRef = useRef<LibreMap | null>(null);
  const libraryRef = useRef<typeof import("maplibre-gl") | null>(null);
  const markerRef = useRef<Marker | null>(null);
  const fitRef = useRef<() => void>(() => {});
  const index = Math.min(selectedIndex, Math.max(0, points.length - 1));
  const selected = points[index];
  const row = selected ? table.rows[selected.rowIndex] : undefined;
  const choose = (next: number) => {
    const point = points[next];
    if (!point) return;
    setSelectedIndex(next);
    setDetailsOpen(true);
    const source = table.sources[table.rows[point.rowIndex].primarySourceIndex];
    if (source) onSelectArtifactId?.(source.artifactId);
  };
  const latest = useRef({ choose });
  latest.current = { choose };

  useEffect(() => {
    setSelectedIndex((current) => {
      const point = points[current];
      if (point && (!selectedArtifactId || table.sources[table.rows[point.rowIndex].primarySourceIndex]?.artifactId === selectedArtifactId)) return current;
      const matching = points.findIndex((candidate) => table.sources[table.rows[candidate.rowIndex].primarySourceIndex]?.artifactId === selectedArtifactId);
      return matching < 0 ? 0 : matching;
    });
  }, [points, table, selectedArtifactId]);

  useEffect(() => {
    const element = container.current;
    if (!element || !points.length) return;
    let disposed = false;
    let map: LibreMap | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let resize: ResizeObserver | undefined;
    let theme: MutationObserver | undefined;
    let local = false;
    setStatus("Loading map…");
    void import("maplibre-gl").then((library) => {
      if (disposed) return;
      libraryRef.current = library;
      library.setWorkerUrl(workerUrl);
      map = new library.Map({ container: element, style: basemapStyle(basemap), center: [points[0].longitude, points[0].latitude],
        zoom: 3, attributionControl: { compact: true } });
      mapRef.current = map;
      installBasemapImageResolver(map);
      map.getCanvas().setAttribute("aria-label", "Artifact location map. Click a pin for row details, or use the location controls below.");
      map.addControl(new library.NavigationControl(), "top-right");
      map.addControl(new library.ScaleControl({ maxWidth: 100, unit: "metric" }), "bottom-left");
      const data: GeoJSON.FeatureCollection<GeoJSON.Point> = { type: "FeatureCollection", features: points.map((point, pointIndex) => ({
        type: "Feature", id: pointIndex, properties: { pointIndex },
        geometry: { type: "Point", coordinates: [point.longitude, point.latitude] },
      })) };
      fitRef.current = () => {
        const bounds = locationBounds(points);
        if (map && bounds) map.fitBounds(bounds, { padding: 55, maxZoom: 16, duration: 300 });
      };
      let fitted = false;
      map.on("style.load", () => {
        if (!map || disposed) return;
        clearTimeout(timer);
        map.addImage("artifact-pin", pinImage());
        map.addSource(SOURCE, { type: "geojson", data, cluster: true, clusterRadius: 45, clusterMaxZoom: 16 });
        map.addLayer({ id: CLUSTERS, type: "circle", source: SOURCE, filter: ["has", "point_count"], paint: {
          "circle-color": "#0d9488", "circle-radius": ["step", ["get", "point_count"], 16, 100, 22, 1000, 28],
          "circle-opacity": 0.8, "circle-stroke-width": 2, "circle-stroke-color": "#ffffff",
        } });
        map.addLayer({ id: PINS, type: "symbol", source: SOURCE, filter: ["!", ["has", "point_count"]], layout: {
          "icon-image": "artifact-pin", "icon-size": 0.85, "icon-anchor": "bottom", "icon-allow-overlap": true,
        } });
        if (!fitted) { fitRef.current(); fitted = true; }
        setStatus(local ? "Basemap unavailable. Location pins remain visible." : "");
        setReadyVersion((version) => version + 1);
      });
      map.on("click", PINS, (event) => {
        const pointIndex = Number(event.features?.[0]?.properties.pointIndex);
        if (Number.isInteger(pointIndex)) latest.current.choose(pointIndex);
      });
      map.on("click", CLUSTERS, (event) => {
        const feature = event.features?.[0];
        if (!map || !feature || feature.geometry.type !== "Point") return;
        const coordinates = feature.geometry.coordinates as [number, number];
        const source = map.getSource(SOURCE) as GeoJSONSource;
        void source.getClusterExpansionZoom(Number(feature.properties.cluster_id)).then((zoom) => {
          if (!disposed) map?.easeTo({ center: coordinates, zoom });
        }).catch(() => {});
      });
      for (const layer of [PINS, CLUSTERS]) {
        map.on("mouseenter", layer, () => { if (map) map.getCanvas().style.cursor = "pointer"; });
        map.on("mouseleave", layer, () => { if (map) map.getCanvas().style.cursor = ""; });
      }
      function fallback() {
        if (disposed || local || !map) return;
        local = true;
        clearTimeout(timer);
        setStatus("Basemap unavailable. Location pins remain visible.");
        map.setStyle(localStyle());
      }
      map.on("error", () => {
        if (disposed || local || !map) return;
        if (!hasBasemapStyle(map)) fallback();
        else setStatus("Some basemap resources could not load. Location pins remain available.");
      });
      timer = setTimeout(fallback, 30_000);
      resize = new ResizeObserver(() => map?.resize());
      resize.observe(element);
      theme = new MutationObserver(() => {
        clearTimeout(timer);
        map?.setStyle(local ? localStyle() : basemapStyle(basemap));
        if (!local) timer = setTimeout(fallback, 30_000);
      });
      theme.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    }).catch((error: unknown) => {
      if (!disposed) setStatus(`Map unavailable. WebGL is required. ${String(error)}`);
    });
    return () => {
      disposed = true;
      clearTimeout(timer);
      resize?.disconnect(); theme?.disconnect();
      markerRef.current?.getPopup()?.remove(); markerRef.current?.remove(); markerRef.current = null;
      map?.remove(); mapRef.current = null; fitRef.current = () => {};
    };
  }, [points, basemap, retry]);

  useEffect(() => {
    const map = mapRef.current;
    const library = libraryRef.current;
    if (!map || !library || !selected || !map.getSource(SOURCE)) return;
    markerRef.current?.getPopup()?.remove(); markerRef.current?.remove();
    const title = document.createElement("div");
    title.style.color = "#111827";
    title.textContent = `${selected.pairLabel} · ${index + 1} / ${points.length}\n${selected.latitude}, ${selected.longitude}`;
    const longitude = selected.longitude + 360 * Math.round((map.getCenter().lng - selected.longitude) / 360);
    markerRef.current = new library.Marker({ color: "#f97316" }).setLngLat([longitude, selected.latitude])
      .setPopup(new library.Popup({ offset: 30, maxWidth: "280px" }).setDOMContent(title)).addTo(map);
    markerRef.current.getElement().addEventListener("click", () => latest.current.choose(index));
    markerRef.current.togglePopup();
    if (!map.getBounds().contains([longitude, selected.latitude])) {
      map.easeTo({ center: [longitude, selected.latitude], zoom: Math.max(map.getZoom(), 12) });
    }
  }, [selected, index, points.length, readyVersion]);

  return <section className="flex h-full min-h-0 flex-col" aria-label="Artifact location preview">
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b px-2 py-1 text-xs">
      <MapPin className="size-3.5" />
      <span>{points.length.toLocaleString()} locations</span>
      {invalidRows > 0 && <span className="text-muted-foreground">{invalidRows.toLocaleString()} rows without valid coordinates</span>}
      <select aria-label="Location map basemap" value={basemap} onChange={(event) => setBasemap(event.target.value as Basemap)}
        className="ml-auto rounded-sm border bg-background px-2 py-1 text-xs">
        <option value="streets">Streets</option><option value="satellite">Satellite</option>
      </select>
      <OpenLocationInBrowserButton latitude={selected?.latitude} longitude={selected?.longitude} />
      {selected && !detailsOpen && <Button variant="outline" size="sm" className="h-7 px-2 text-xs"
        onClick={() => setDetailsOpen(true)} aria-expanded={false} aria-controls="artifact-location-details">Show details</Button>}
      <Button variant="ghost" size="icon-xs" aria-label="Fit all locations" onClick={() => fitRef.current()}><LocateFixed className="size-3.5" /></Button>
      <Button variant="ghost" size="icon-xs" aria-label="Reload location map" onClick={() => setRetry((value) => value + 1)}><RefreshCw className="size-3.5" /></Button>
    </div>
    {points.length ? <div className="relative min-h-40 flex-1">
      {/* MapLibre's unlayered CSS sets position:relative; override it as on the drone map. */}
      <div ref={container} className="absolute! inset-0" />
      {status && <div role="status" className="pointer-events-none absolute bottom-7 left-2 right-12 rounded-sm border bg-background/95 px-2 py-1 text-xs">{status}</div>}
    </div> : <div role="status" className="grid min-h-32 flex-1 place-items-center p-4 text-center text-xs text-muted-foreground">
      {pairs.length ? "No valid coordinate pairs were found. Empty, out-of-range and 0,0 coordinates are excluded." : "No matching latitude and longitude columns were found."}
    </div>}
    {selected && row && detailsOpen && <div id="artifact-location-details" className="flex max-h-[45%] min-h-24 flex-col border-t">
      <div className="flex shrink-0 items-center gap-2 border-b bg-muted/20 px-2 py-1 text-xs">
        <span className="font-medium">Location details · {selected.pairLabel}</span>
        <div className="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="icon-xs" aria-label="Previous location" disabled={index === 0} onClick={() => choose(index - 1)}><ChevronLeft className="size-3.5" /></Button>
          <input aria-label="Location number" type="number" min={1} max={points.length} value={index + 1}
            className="w-20 rounded-sm border bg-background px-1 py-0.5 text-xs" onChange={(event) => {
              const next = Number(event.target.value) - 1;
              if (Number.isInteger(next)) choose(next);
            }} />
          <span className="text-muted-foreground">/ {points.length.toLocaleString()}</span>
          <Button variant="ghost" size="icon-xs" aria-label="Next location" disabled={index === points.length - 1} onClick={() => choose(index + 1)}><ChevronRight className="size-3.5" /></Button>
          <Button variant="ghost" size="icon-xs" aria-label="Close location details" title="Close details"
            onClick={() => setDetailsOpen(false)}><X className="size-3.5" /></Button>
        </div>
      </div>
      <div className="min-h-0 overflow-auto px-2 text-xs">
        <div className="border-b py-2 font-mono">Latitude: {selected.latitude} · Longitude: {selected.longitude}</div>
        <dl className="divide-y">
          {table.columns.map((column) => <div key={column.key} className="grid grid-cols-[minmax(7rem,30%)_minmax(0,1fr)] gap-2 py-1.5">
            <dt className="break-words text-muted-foreground">{column.label}</dt>
            <dd className="whitespace-pre-wrap break-words">{formatArtifactTableCell(row.values[column.key], column) || "-"}</dd>
          </div>)}
        </dl>
        <div className="border-t py-2">
          <div className="mb-1 font-medium">Sources · {row.occurrenceCount.toLocaleString()} occurrences</div>
          {getCustomTableRowSources(table, row).map((source) => <div key={source.artifactId} className="break-all font-mono text-[11px]">{source.filePath || source.artifactId}</div>)}
        </div>
      </div>
    </div>}
  </section>;
}
