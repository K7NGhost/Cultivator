import { useMemo, useRef, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { Download, Layers, LocateFixed, LoaderCircle, PanelLeft, PanelRight, Plug, RefreshCw, X } from "lucide-react";
import type { PanelImperativeHandle } from "react-resizable-panels";
import { Button } from "@/components/ui/button";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { useCases, type CaseRecord } from "@/features/cases";
import type { Basemap } from "@/lib/maps/mapStyles";
import { MapCanvas, type MapHandle } from "../components/MapCanvas";
import { FeatureInspector } from "../components/FeatureInspector";
import { DateRangeFilter } from "../components/DateRangeFilter";
import { useMapLayers } from "../hooks/useMapLayers";
import { featureAtSourceIndex, LAYER_COLORS } from "../lib/mapFeatures";
import { filterMapDates, type MapDateFilter } from "../lib/dateFilter";
import type { FeatureSelection, VisibleMapLayer } from "../types";
import { useTimeZone } from "@/features/settings/timeZoneSettings";

export function MapPage() {
  const { activeCase } = useCases();
  if (!activeCase) return <div className="flex items-center gap-2 border-b p-2 text-xs text-muted-foreground">
    Open a case to inspect map files.<Button asChild variant="outline" size="sm" className="h-7 text-xs"><Link to="/case">Open case</Link></Button>
  </div>;
  return <MapWorkspace key={activeCase.databasePath} caseRecord={activeCase} />;
}

export function MapWorkspace({ caseRecord }: { caseRecord: CaseRecord }) {
  const timeZone = useTimeZone();
  const active = useLocation().pathname === "/map";
  const { layers, visible, setVisible, data, refreshing, loading, error, refresh } = useMapLayers(caseRecord.databasePath, active);
  const [selected, setSelected] = useState<FeatureSelection | null>(null);
  const [inspectId, setInspectId] = useState("");
  const [query, setQuery] = useState("");
  const [basemap, setBasemap] = useState<Basemap>("streets");
  const [dates, setDates] = useState<MapDateFilter>({ from: "", to: "", includeUndated: false });
  const [status, setStatus] = useState("Loading map…");
  const [layersOpen, setLayersOpen] = useState(true);
  const [featuresOpen, setFeaturesOpen] = useState(true);
  const [exporting, setExporting] = useState(false);
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const layersPanel = useRef<PanelImperativeHandle | null>(null);
  const featuresPanel = useRef<PanelImperativeHandle | null>(null);
  const map = useRef<MapHandle>(null);
  const loadedLayers = useMemo<VisibleMapLayer[]>(() => layers.flatMap((layer, index) => visible.includes(layer.id) && data[layer.id]
    ? [{ ...layer, ...data[layer.id], color: LAYER_COLORS[index % LAYER_COLORS.length] }] : []), [layers, visible, data]);
  const dateView = useMemo(() => filterMapDates(loadedLayers, dates, timeZone), [loadedLayers, dates, timeZone]);
  const shown = dateView.layers;
  const inspected = shown.find((layer) => layer.id === selected?.layerId) ?? shown.find((layer) => layer.id === inspectId) ?? shown[0];
  const selection = selected && shown.some((layer) => layer.id === selected.layerId && featureAtSourceIndex(layer, selected.index)) ? selected : null;
  const filtered = layers.filter((layer) => `${layer.label} ${layer.filePath}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  function selectFeature(value: FeatureSelection) {
    setSelected(value);
    featuresPanel.current?.expand();
  }
  async function exportHtml() {
    setExporting(true);
    setExportMessage(null);
    try {
      const { exportMapHtml } = await import("../export/exportMapHtml");
      const destination = await exportMapHtml(caseRecord.databasePath, {
        title: `${caseRecord.name} — Map`, layers: shown, dates, basemap, timeZone,
      });
      if (destination) setExportMessage(`Saved HTML map: ${destination}`);
    } catch (caught) {
      setExportMessage(`HTML export failed: ${String(caught)}`);
    } finally { setExporting(false); }
  }
  return <div className="flex h-full min-h-0 min-w-0 flex-col bg-background text-xs">
    <div className="flex min-h-10 shrink-0 flex-wrap items-center gap-1 border-b px-2 py-1" aria-label="Map actions">
      <Button size="icon" variant="ghost" className="size-7" aria-label="Refresh map layers" disabled={refreshing} onClick={() => void refresh()}>
        <RefreshCw className={`size-3.5 ${refreshing ? "animate-spin" : ""}`} /></Button>
      <Button asChild size="sm" variant="ghost" className="h-7 px-2 text-xs"><Link to="/plugins"><Plug className="size-3.5" />Run plugin</Link></Button>
      <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" disabled={!shown.length} onClick={() => map.current?.fitAll()}><LocateFixed className="size-3.5" />Fit visible layers</Button>
      <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" disabled={!dateView.matched || loading || exporting}
        onClick={() => void exportHtml()}>{exporting ? <LoaderCircle className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}Export HTML</Button>
      <Button size="sm" variant={layersOpen ? "secondary" : "ghost"} className="h-7 px-2 text-xs"
        aria-label="Toggle layers sidebar" aria-expanded={layersOpen} aria-controls="map-layers-sidebar"
        onClick={() => layersOpen ? layersPanel.current?.collapse() : layersPanel.current?.expand()}><PanelLeft className="size-3.5" />Layers</Button>
      <Button size="sm" variant={featuresOpen ? "secondary" : "ghost"} className="h-7 px-2 text-xs"
        aria-label="Toggle features sidebar" aria-expanded={featuresOpen} aria-controls="map-features-sidebar"
        onClick={() => featuresOpen ? featuresPanel.current?.collapse() : featuresPanel.current?.expand()}><PanelRight className="size-3.5" />Features</Button>
      {loading && <span role="status" className="ml-1 flex items-center gap-1 text-muted-foreground"><LoaderCircle className="size-3 animate-spin" />Loading layer…</span>}
      <select aria-label="Map basemap" className="select select-xs ml-auto w-28 rounded-sm border-border bg-background" value={basemap}
        onChange={(event) => setBasemap(event.target.value === "satellite" ? "satellite" : "streets")}>
        <option value="streets">Streets</option><option value="satellite">Satellite</option></select>
    </div>
    <DateRangeFilter value={dates} onChange={setDates} error={dateView.error} timeZone={timeZone} />
    {exportMessage && <p role="status" className="shrink-0 border-b px-2 py-1 text-muted-foreground select-text break-all">{exportMessage}</p>}
    {error && <p role="alert" className="border-b bg-destructive/10 px-2 py-1 text-destructive">{error}</p>}
    <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
      <ResizablePanel id="map-sidebar" panelRef={layersPanel} collapsible collapsedSize="0px"
        defaultSize="256px" minSize="208px" maxSize="45%" onResize={(size) => setLayersOpen(size.asPercentage > 0.01)}>
      <aside id="map-layers-sidebar" className="flex h-full min-h-0 min-w-0 flex-col" aria-label="Map layer library" aria-hidden={!layersOpen} inert={!layersOpen}>
        <div className="flex h-8 shrink-0 items-center gap-2 border-b px-2 font-medium"><Layers className="size-3.5" />Layers<span className="ml-auto text-muted-foreground">{visible.length} / {layers.length}</span>
          <Button size="icon" variant="ghost" className="size-6" aria-label="Close layers sidebar" onClick={() => layersPanel.current?.collapse()}><X className="size-3.5" /></Button></div>
        <input className="input input-xs m-2 w-auto shrink-0 rounded-sm" aria-label="Search map layers" placeholder="Find a layer…" value={query} onChange={(event) => setQuery(event.target.value)} />
        <div className="min-h-0 flex-1 overflow-auto">
          {!layers.length && <p className="p-2 text-muted-foreground">{refreshing ? "Loading layers…" : "Add a datasource containing KML, GPX, or GeoJSON files and run Map Files from Plugins."}</p>}
          {filtered.map((layer) => <div key={layer.id} className={`flex items-start gap-2 px-2 py-1.5 ${inspected?.id === layer.id ? "bg-accent/60" : ""}`}>
            <input type="checkbox" className="checkbox checkbox-xs mt-0.5" checked={visible.includes(layer.id)} aria-label={`Show ${layer.label}`} onChange={(event) => {
              const checked = event.target.checked;
              setVisible((current) => checked ? [...current, layer.id] : current.filter((id) => id !== layer.id));
              if (checked) { setInspectId(layer.id); setSelected(null); }
              else if (selected?.layerId === layer.id) setSelected(null);
            }} />
            <button type="button" className="min-w-0 flex-1 text-left" title={layer.filePath} onClick={() => { setInspectId(layer.id); setSelected(null); featuresPanel.current?.expand(); }}>
              <span className="block truncate">{layer.label}</span><span className="block text-[10px] text-muted-foreground">{layer.format.toUpperCase()} · {dateView.active && shown.some((item) => item.id === layer.id)
                ? `${shown.find((item) => item.id === layer.id)!.featureCount.toLocaleString()} / ` : ""}{layer.featureCount.toLocaleString()} features</span>
            </button>
            <Button size="icon" variant="ghost" className="size-6 shrink-0" disabled={!shown.some((item) => item.id === layer.id)} aria-label={`Fit ${layer.label}`} onClick={() => map.current?.fitLayer(layer.id)}><LocateFixed className="size-3.5" /></Button>
          </div>)}
        </div>
      </aside>
      </ResizablePanel>
      <ResizableHandle withHandle aria-label="Resize layers sidebar" disabled={!layersOpen} className={layersOpen ? undefined : "hidden"} />
      <ResizablePanel id="map-viewport" minSize="25%">
      <main className="relative h-full min-h-0 min-w-0 bg-muted" aria-label="General map workspace">
        <MapCanvas ref={map} layers={shown} basemap={basemap} selected={selection} onSelect={selectFeature} onStatus={setStatus} />
        {dateView.active && !dateView.error && dateView.total > 0 && dateView.matched === 0 && <p role="status"
          className="pointer-events-none absolute left-2 top-2 rounded-sm border bg-background/95 px-2 py-1">No features match this date range.</p>}
        {status && <div role="status" className="absolute bottom-8 left-2 max-w-sm rounded-sm border bg-background/95 px-2 py-1">{status}
          {!status.startsWith("Loading") && <button type="button" className="ml-2 underline" onClick={() => map.current?.retry()}>Retry basemap</button>}</div>}
      </main>
      </ResizablePanel>
      <ResizableHandle withHandle aria-label="Resize features sidebar" disabled={!featuresOpen} className={featuresOpen ? undefined : "hidden"} />
      <ResizablePanel id="map-features" panelRef={featuresPanel} collapsible collapsedSize="0px"
        defaultSize="280px" minSize="208px" maxSize="45%" onResize={(size) => setFeaturesOpen(size.asPercentage > 0.01)}>
        <aside id="map-features-sidebar" className="flex h-full min-h-0 min-w-0 flex-col" aria-label="Map feature browser" aria-hidden={!featuresOpen} inert={!featuresOpen}>
          <div className="flex h-8 shrink-0 items-center gap-2 border-b px-2 font-medium">Features
            <span className="ml-auto text-muted-foreground">{inspected?.featureCount.toLocaleString() ?? 0}</span>
            <Button size="icon" variant="ghost" className="size-6" aria-label="Close features sidebar" onClick={() => featuresPanel.current?.collapse()}><X className="size-3.5" /></Button>
          </div>
          {inspected ? <>
            <select className="select select-xs m-2 w-auto shrink-0 rounded-sm border-border bg-background" aria-label="Features layer"
              value={inspected.id} onChange={(event) => { setInspectId(event.target.value); setSelected(null); }}>
              {shown.map((layer) => <option key={layer.id} value={layer.id}>{layer.label}</option>)}
            </select>
            <div className="border-b px-2 py-1 text-[10px] text-muted-foreground break-all select-text" title={inspected.filePath}>
              <span className="mr-1 inline-block size-2 rounded-full" style={{ backgroundColor: inspected.color }} />{inspected.filePath}
            </div>
            <FeatureInspector key={inspected.id} layer={inspected} selected={selection} onSelect={selectFeature} onFit={() => map.current?.fitSelection()} />
          </> : <p className="p-2 text-muted-foreground">Enable a layer to browse its features.</p>}
        </aside>
      </ResizablePanel>
    </ResizablePanelGroup>
    <div className="flex h-6 shrink-0 items-center border-t px-2 text-[10px] text-muted-foreground">
      {shown.length} visible layers · {dateView.active ? `${dateView.matched.toLocaleString()} / ${dateView.total.toLocaleString()}` : dateView.total.toLocaleString()} features
      {dateView.active && dateView.undated > 0 && <span className="ml-2">{dateView.undated.toLocaleString()} without dates/times {dates.includeUndated ? "included" : "excluded"}</span>}
      <span className="ml-auto">WGS84 · longitude, latitude</span>
    </div>
  </div>;
}
