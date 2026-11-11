import { useMemo, useState } from "react";
import { LocateFixed } from "lucide-react";
import { Button } from "@/components/ui/button";
import { featureAtSourceIndex, featureName, sourceFeatureIndex } from "../lib/mapFeatures";
import type { FeatureSelection, VisibleMapLayer } from "../types";
import { useTimeZone } from "@/features/settings/timeZoneSettings";
import { timestampDisplayValue } from "@/lib/timeZone";

export function FeatureInspector({ layer, selected, onSelect, onFit }: {
  layer: VisibleMapLayer; selected: FeatureSelection | null; onSelect: (value: FeatureSelection) => void; onFit: () => void;
}) {
  const timeZone = useTimeZone();
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const matches = useMemo(() => layer.collection.features.map((feature, index) => ({ feature,
    index: sourceFeatureIndex(layer, index), name: featureName(feature, sourceFeatureIndex(layer, index)) }))
    .filter((item) => !query || item.name.toLocaleLowerCase().includes(query.toLocaleLowerCase())), [layer, query]);
  const current = selected?.layerId === layer.id ? featureAtSourceIndex(layer, selected.index) : undefined;
  const currentPage = Math.min(page, Math.max(0, Math.ceil(matches.length / 100) - 1));
  const properties = current?.properties ?? {};
  return <>
    <input className="input input-xs m-2 w-auto shrink-0 rounded-sm" aria-label="Search map features" placeholder="Find a feature…" value={query}
      onChange={(event) => { setQuery(event.target.value); setPage(0); }} />
    <div className="min-h-20 max-h-56 overflow-auto border-b" aria-label="Map features">
      {matches.slice(currentPage * 100, (currentPage + 1) * 100).map(({ feature, index, name }) => <button key={index} type="button"
        aria-pressed={selected?.layerId === layer.id && selected.index === index}
        className={`block w-full px-2 py-1 text-left hover:bg-accent ${selected?.layerId === layer.id && selected.index === index ? "bg-accent" : ""}`}
        onClick={() => onSelect({ layerId: layer.id, index })}>
        <span className="block truncate" title={name}>{name}</span><span className="text-[10px] text-muted-foreground">{feature.geometry.type}</span>
      </button>)}
      {!matches.length && <p className="p-2 text-muted-foreground">No matching features.</p>}
    </div>
    {matches.length > 100 && <div className="flex items-center justify-between border-b px-2 py-1">
      <Button size="sm" variant="ghost" className="h-6 px-1 text-xs" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>Previous</Button>
      <span>{currentPage + 1} / {Math.ceil(matches.length / 100)}</span>
      <Button size="sm" variant="ghost" className="h-6 px-1 text-xs" disabled={(currentPage + 1) * 100 >= matches.length} onClick={() => setPage(currentPage + 1)}>Next</Button>
    </div>}
    <div className="min-h-0 flex-1 overflow-auto p-2">
      {current ? <>
        <div className="mb-2 flex items-start gap-1"><strong className="min-w-0 flex-1 break-words">{featureName(current, selected?.index ?? 0)}</strong>
          <Button size="icon" variant="ghost" className="size-6 shrink-0" aria-label="Zoom to selected feature" onClick={onFit}><LocateFixed className="size-3.5" /></Button></div>
        <p className="mb-2 text-muted-foreground">{current.geometry.type}{current.id !== undefined ? ` · ID ${current.id}` : ""}</p>
        {current.geometry.type === "Point" && <dl className="mb-3 border-y py-2 font-mono text-[11px] select-text">
          <div>Longitude: {current.geometry.coordinates[0]}</div><div>Latitude: {current.geometry.coordinates[1]}</div>
          {current.geometry.coordinates[2] !== undefined && <div>Elevation: {current.geometry.coordinates[2]} m</div>}
        </dl>}
        <dl className="space-y-2">{Object.entries(properties).map(([key, value]: [string, unknown]) => <div key={key}>
          <dt className="font-medium text-muted-foreground">{key}</dt><dd className="whitespace-pre-wrap break-words select-text" title={typeof value === "string" ? value : undefined}>{typeof value === "string" ? timestampDisplayValue(value, timeZone) as string : JSON.stringify(timestampDisplayValue(value, timeZone), null, 2)}</dd>
        </div>)}</dl>
      </> : <p className="text-muted-foreground">Select a feature on the map or in the list to inspect its properties.</p>}
      {!!layer.warnings.length && <details className="mt-3 border-t pt-2"><summary>{layer.warnings.length} parsing warnings</summary>
        <ul className="mt-1 space-y-1 text-muted-foreground">{layer.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul></details>}
    </div>
  </>;
}
