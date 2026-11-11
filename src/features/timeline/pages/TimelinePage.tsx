import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { ArrowDownUp, ChevronLeft, ChevronRight, Clock3, ExternalLink, LoaderCircle, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { useCases, type CaseRecord } from "@/features/cases";
import { listDataSources, type DataSourceRecord } from "@/features/datasources";
import { cn } from "@/lib/utils";
import { useTimeZone } from "@/features/settings/timeZoneSettings";
import { formatTimestamp } from "@/lib/timeZone";
import { syncTimeline, timelinePage, timelineDetail, type TimelineSummary, type TimelineFilter,
  type TimelinePageData, type TimelineDetail } from "../services/timelineRepository";

const PAGE_SIZE = 200;
const EMPTY_FILTER: TimelineFilter = { search: "", datasourceId: "", label: "", basis: "", from: "", to: "", descending: false };
const control = "h-7 min-w-0 rounded-sm border bg-background px-2 text-xs";
const errorMessage = (error: unknown) => error instanceof Error ? error.message : String(error);
const valueText = (value: unknown): string => value === null || value === undefined ? "-" : typeof value === "object" ? JSON.stringify(value, null, 2) : String(value);

export function TimelinePage() {
  const { activeCase } = useCases();
  if (!activeCase) return <div className="flex items-center gap-2 border-b p-2 text-xs text-muted-foreground">
    Open a case to view its artifact timeline.
    <Button asChild variant="outline" size="sm" className="h-7 text-xs"><Link to="/case">Open case</Link></Button>
  </div>;
  return <TimelineWorkspace key={activeCase.databasePath} caseRecord={activeCase} />;
}

export function TimelineWorkspace({ caseRecord }: { caseRecord: CaseRecord }) {
  const timeZone = useTimeZone();
  const displayTime = (event: TimelineDetail["event"]) => event.timeBasis === "utc"
    ? formatTimestamp(event.displayTime.replace(/ UTC$/, "Z"), timeZone) : event.displayTime;
  const active = useLocation().pathname === "/timeline";
  const [summary, setSummary] = useState<TimelineSummary | null>(null);
  const [sources, setSources] = useState<DataSourceRecord[]>([]);
  const [filter, setFilter] = useState<TimelineFilter>(EMPTY_FILTER);
  const [search, setSearch] = useState("");
  const [offset, setOffset] = useState(0);
  const [version, setVersion] = useState(0);
  const [page, setPage] = useState<TimelinePageData | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<TimelineDetail | null>(null);
  const [building, setBuilding] = useState(false);
  const [paging, setPaging] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [error, setError] = useState("");
  const [detailError, setDetailError] = useState("");
  const generation = useRef(0);

  const refresh = useCallback(async () => {
    const request = ++generation.current;
    setBuilding(true); setError(""); setSelectedId(null); setDetail(null);
    try {
      const [result, datasources] = await Promise.all([syncTimeline(caseRecord.databasePath), listDataSources(caseRecord.databasePath, caseRecord.id)]);
      if (request !== generation.current) return;
      setSummary(result); setSources(datasources); setOffset(0); setVersion((value) => value + 1);
    } catch (error) { if (request === generation.current) setError(errorMessage(error)); }
    finally { if (request === generation.current) setBuilding(false); }
  }, [caseRecord.databasePath, caseRecord.id]);

  useEffect(() => {
    if (!active) return;
    void refresh();
    const listener = listen("plugin-run-finished", () => { void refresh(); });
    return () => { generation.current++; void listener.then((stop) => stop()).catch(() => {}); };
  }, [active, refresh]);

  useEffect(() => {
    const timer = window.setTimeout(() => { setFilter((current) => current.search === search ? current : { ...current, search }); setOffset(0); }, 300);
    return () => window.clearTimeout(timer);
  }, [search]);

  useEffect(() => {
    if (!summary || !active || building) return;
    let disposed = false;
    setPaging(true); setError(""); setSelectedId(null); setDetail(null);
    void timelinePage(caseRecord.databasePath, filter, offset, PAGE_SIZE).then((result) => {
      if (disposed) return;
      setPage(result); setSelectedId(result.items[0]?.id ?? null);
    }).catch((error: unknown) => { if (!disposed) { setPage(null); setError(errorMessage(error)); } })
      .finally(() => { if (!disposed) setPaging(false); });
    return () => { disposed = true; };
  }, [caseRecord.databasePath, filter, offset, version, summary, active, building]);

  useEffect(() => {
    setDetail(null); setDetailError("");
    if (selectedId === null || !active) { setDetailLoading(false); return; }
    let disposed = false;
    setDetailLoading(true);
    void timelineDetail(caseRecord.databasePath, selectedId).then((result) => { if (!disposed) setDetail(result); })
      .catch((error: unknown) => { if (!disposed) setDetailError(errorMessage(error)); })
      .finally(() => { if (!disposed) setDetailLoading(false); });
    return () => { disposed = true; };
  }, [caseRecord.databasePath, selectedId, active]);

  function updateFilter(key: keyof TimelineFilter, value: string | boolean) {
    setFilter((current) => ({ ...current, [key]: value })); setOffset(0);
  }
  const sourceName = (id: string) => sources.find((source) => source.id === id)?.name ?? id;

  return <section className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden" aria-label="Artifact timeline">
    <header className="flex shrink-0 flex-wrap items-center gap-2 border-b px-3 py-2">
      <Clock3 className="size-4 text-primary" /><h1 className="text-sm font-semibold">Timeline</h1>
      <span className="text-xs text-muted-foreground">Events from saved artifacts</span>
      <span className="ml-auto text-xs text-muted-foreground">{summary ? `${summary.events.toLocaleString()} events · ${summary.datedArtifacts.toLocaleString()} dated artifacts` : ""}</span>
      <Button variant="outline" size="sm" className="h-7 text-xs" disabled={building} onClick={() => void refresh()}>
        {building ? <LoaderCircle className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />} Refresh
      </Button>
    </header>
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b bg-muted/15 px-3 py-2">
      <input className={cn(control, "w-64")} aria-label="Search timeline" placeholder="Search events, summaries or paths…" value={search} onChange={(event) => setSearch(event.target.value)} />
      <select className={cn(control, "max-w-56")} aria-label="Timeline datasource" value={filter.datasourceId} onChange={(event) => updateFilter("datasourceId", event.target.value)}>
        <option value="">All datasources</option>{sources.map((source) => <option key={source.id} value={source.id}>{source.name}</option>)}
      </select>
      <select className={cn(control, "max-w-64")} aria-label="Timeline artifact" value={filter.label} onChange={(event) => updateFilter("label", event.target.value)}>
        <option value="">All artifacts</option>{page?.labels.map((label) => <option key={label} value={label}>{label}</option>)}
      </select>
      <select className={control} aria-label="Timestamp timezone" value={filter.basis} onChange={(event) => updateFilter("basis", event.target.value)}>
        <option value="">All timestamp types</option><option value="utc">Known timezone (UTC)</option>
        <option value="unspecified">Timezone unspecified</option><option value="date">Date only</option>
      </select>
      <label className="flex items-center gap-1 text-xs">From (UTC) <input className={control} type="date" aria-label="Timeline start date" value={filter.from} onChange={(event) => updateFilter("from", event.target.value)} /></label>
      <label className="flex items-center gap-1 text-xs">To (UTC) <input className={control} type="date" aria-label="Timeline end date" value={filter.to} min={filter.from || undefined} onChange={(event) => updateFilter("to", event.target.value)} /></label>
      <Button variant="outline" size="sm" className="h-7 text-xs" onClick={() => updateFilter("descending", !filter.descending)}><ArrowDownUp className="size-3.5" />{filter.descending ? "Newest first" : "Oldest first"}</Button>
      <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => { setSearch(""); setFilter(EMPTY_FILTER); setOffset(0); }}>Clear filters</Button>
    </div>
    <div className="flex shrink-0 flex-wrap gap-x-4 gap-y-1 border-b px-3 py-1 text-[11px] text-muted-foreground">
      <span>Display timezone: {timeZone}. Timeline date filters use UTC. Unspecified times sort by their recorded clock; date-only events have no time of day.</span>
      {summary && summary.undated > 0 && <span>{summary.undated.toLocaleString()} artifacts have no recognized timestamp.</span>}
    </div>
    {error && <div role="alert" className="shrink-0 border-b px-3 py-2 text-xs text-destructive">{error}</div>}
    {building && <div role="status" className="flex shrink-0 items-center gap-2 border-b px-3 py-2 text-xs"><LoaderCircle className="size-3.5 animate-spin" />Updating timeline from saved artifacts…</div>}
    <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
      <ResizablePanel defaultSize="66%" minSize="35%">
        <div className="flex h-full min-h-0 flex-col">
          <div className="min-h-0 flex-1 overflow-auto" aria-busy={paging || building}>
            {page?.items.length ? <table className="w-full border-collapse text-left text-xs">
              <thead className="sticky top-0 z-10 bg-background text-[11px] text-muted-foreground"><tr>
                {['Time', 'Event', 'Artifact', 'Summary', 'Source'].map((label) => <th key={label} className="border-b px-2 py-2 font-medium">{label}</th>)}
              </tr></thead>
              <tbody>{page.items.map((event) => <tr key={event.id} onClick={() => setSelectedId(event.id)} className={cn("cursor-pointer border-b hover:bg-muted/40", selectedId === event.id && "bg-primary/10")}>
                <td className="whitespace-nowrap px-2 py-2 align-top font-mono text-[11px]"><button type="button" className="text-left hover:underline" onClick={() => setSelectedId(event.id)} aria-pressed={selectedId === event.id}>
                  {displayTime(event)}<span className="block font-sans text-[10px] text-muted-foreground">{event.timeBasis === "unspecified" ? "Timezone unspecified" : event.timeBasis === "date" ? "Date only" : timeZone}</span>
                </button></td>
                <td className="px-2 py-2 align-top">{event.eventType}</td>
                <td className="max-w-52 px-2 py-2 align-top"><div className="truncate" title={event.label}>{event.label}</div></td>
                <td className="max-w-72 px-2 py-2 align-top"><div className="line-clamp-2 break-words" title={event.summary}>{event.summary || "-"}</div></td>
                <td className="max-w-48 px-2 py-2 align-top text-[11px] text-muted-foreground"><div className="truncate" title={event.filePath}>{sourceName(event.datasourceId)}</div><div className="truncate" title={event.filePath}>{event.filePath}</div></td>
              </tr>)}</tbody>
            </table> : <div role="status" className="grid h-full min-h-40 place-items-center p-6 text-center text-xs text-muted-foreground">
              {paging || building ? "Loading events…" : summary?.events ? "No events match these filters." : "No recognized artifact dates yet. Saved timestamp columns appear here automatically."}
            </div>}
          </div>
          <footer className="flex shrink-0 items-center justify-between gap-2 border-t px-2 py-1 text-xs">
            <span>{page?.total ? `${(offset + 1).toLocaleString()}–${Math.min(offset + PAGE_SIZE, page.total).toLocaleString()} of ${page.total.toLocaleString()} events` : "0 events"}</span>
            <div className="flex gap-1">
              <Button variant="outline" size="icon-xs" aria-label="Previous timeline page" disabled={offset === 0 || paging || building} onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}><ChevronLeft className="size-3.5" /></Button>
              <Button variant="outline" size="icon-xs" aria-label="Next timeline page" disabled={!page || offset + PAGE_SIZE >= page.total || paging || building} onClick={() => setOffset(offset + PAGE_SIZE)}><ChevronRight className="size-3.5" /></Button>
            </div>
          </footer>
        </div>
      </ResizablePanel>
      <ResizableHandle withHandle />
      <ResizablePanel defaultSize="34%" minSize="22%">
        <aside className="h-full overflow-auto" aria-label="Timeline event details">
          {detailLoading ? <div role="status" className="p-3 text-xs text-muted-foreground">Loading event details…</div> : detailError ? <div role="alert" className="p-3 text-xs text-destructive">{detailError}</div> : detail ? <>
            <div className="space-y-2 border-b p-3">
              <div className="text-sm font-semibold">{detail.event.label}</div>
              <div className="font-mono text-xs">{displayTime(detail.event)}</div>
              <div className="text-xs text-muted-foreground">{detail.event.eventType} · {detail.event.timeBasis === "utc" ? "Known timezone" : detail.event.timeBasis === "date" ? "Date only" : "Timezone unspecified"}</div>
              <Button asChild variant="outline" size="sm" className="h-7 text-xs"><Link to={`/artifacts?artifactId=${encodeURIComponent(detail.event.artifactId)}`}><ExternalLink className="size-3.5" />Open artifact</Link></Button>
            </div>
            <dl className="divide-y px-3 text-xs">
              {[["Timestamp field", detail.event.fieldKey], ["Original timestamp", valueText(detail.rawTime)], ["Datasource", sourceName(detail.event.datasourceId)],
                ["Plugin", detail.event.pluginId], ["Source file", detail.event.filePath], ["Artifact ID", detail.event.artifactId],
                ...(detail.event.rowIndex >= 0 ? [["Table row", String(detail.event.rowIndex + 1)]] : [])].map(([label, value]) => <div key={label} className="grid grid-cols-[7rem_minmax(0,1fr)] gap-2 py-2"><dt className="text-muted-foreground">{label}</dt><dd className="break-words font-mono text-[11px]">{value}</dd></div>)}
            </dl>
            <div className="border-t p-3"><h2 className="mb-2 text-xs font-semibold">Original record</h2>
              <dl className="divide-y text-xs">{Object.entries(detail.row && typeof detail.row === "object" ? detail.row : { value: detail.row }).map(([key, value]) => <div key={key} className="grid grid-cols-[minmax(6rem,30%)_minmax(0,1fr)] gap-2 py-2"><dt className="break-words text-muted-foreground">{key}</dt><dd className="whitespace-pre-wrap break-words font-mono text-[11px]">{valueText(value)}</dd></div>)}</dl>
            </div>
            <details className="border-t p-3 text-xs"><summary className="cursor-pointer font-medium">Source provenance</summary><pre className="mt-2 whitespace-pre-wrap break-all text-[11px]">{JSON.stringify({ parser: detail.parser, source: detail.source, sources: detail.sources, provenance: detail.provenance }, null, 2)}</pre></details>
          </> : <div className="grid h-full place-items-center p-6 text-center text-xs text-muted-foreground">Select an event to inspect its original timestamp, record and source.</div>}
        </aside>
      </ResizablePanel>
    </ResizablePanelGroup>
  </section>;
}
