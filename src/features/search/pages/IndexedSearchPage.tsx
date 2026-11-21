import { useEffect, useMemo, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Link } from "react-router-dom";
import { ChevronLeft, ChevronRight, Database, LoaderCircle, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { useCases } from "@/features/cases";
import type { IndexingStatus } from "@/features/settings/indexingSettings";
import { indexedCategoryName, indexedMaxOffset, indexedPageSize, searchCaseIndex, type IndexCategory, type IndexedHit, type IndexedSearchResult } from "../services/indexedSearchRepository";
import { useTimeZone } from "@/features/settings/timeZoneSettings";
import { indexedRecordDisplay } from "../utils/indexedRecordDisplay";
import { IndexedRecordPreview } from "../components/IndexedRecordPreview";

export function IndexedSearchPage({ active }: { active: boolean }) {
  const { activeCase } = useCases();
  const database = activeCase?.databasePath ?? null;
  const [query, setQuery] = useState("");
  const [semantic,setSemantic]=useState(false);
  const [category, setCategory] = useState<IndexCategory>("all");
  const [submitted, setSubmitted] = useState<{ query: string; category: IndexCategory; semantic:boolean } | null>(null);
  const [result, setResult] = useState<IndexedSearchResult | null>(null);
  const [selected, setSelected] = useState<IndexedHit | null>(null);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [status, setStatus] = useState<IndexingStatus | null>(null);
  const [requesting, setRequesting] = useState(false);
  const [notice, setNotice] = useState("");
  const request = useRef(0);
  const timeZone = useTimeZone();
  const readableHits = useMemo(() => result?.hits.map(hit => ({ hit, display: indexedRecordDisplay(hit, timeZone) })) ?? [], [result, timeZone]);
  const selectedDisplay = useMemo(() => selected ? indexedRecordDisplay(selected, timeZone) : null, [selected, timeZone]);
  useEffect(() => () => { request.current++; }, []);
  useEffect(() => {
    if (!active || !database || !isTauri()) return;
    let disposed = false;
    const poll = async () => {
      try {
        const next = await invoke<IndexingStatus>("case_indexer_status");
        if (!disposed && next.database === database) setStatus(next);
      } catch (error) { if (!disposed) setNotice(String(error)); }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 3000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [active, database]);

  async function search(nextQuery: string, nextCategory: IndexCategory, nextOffset: number, useSemantic=semantic) {
    if (!database || !nextQuery.trim() || loading) return;
    const id = ++request.current;
    setLoading(true); setError("");
    try {
      const next = await searchCaseIndex(database, nextQuery.trim(), nextCategory, nextOffset,useSemantic);
      if (request.current !== id) return;
      setResult(next); setSelected(next.hits[0] ?? null); setOffset(nextOffset);
      setSubmitted({ query: nextQuery.trim(), category: nextCategory,semantic:useSemantic });
    } catch (error) { if (request.current === id) setError(String(error)); }
    finally { if (request.current === id) setLoading(false); }
  }

  return <div className="flex h-full min-h-0 min-w-0 flex-col">
    <form className="flex shrink-0 flex-wrap items-center gap-2 border-b p-2" onSubmit={event => { event.preventDefault(); void search(query, category, 0); }}>
      <Database className="size-3.5 shrink-0 text-muted-foreground" />
      <Input aria-label="Indexed search query" className="h-8 min-w-48 flex-1 rounded-sm text-xs" placeholder="Search indexed files and artifactsâ€¦" maxLength={4096} value={query} onChange={event => setQuery(event.target.value)} />
      <select aria-label="Indexed search method" className="h-8 rounded-sm border bg-background px-2 text-xs" value={semantic?"semantic":"keyword"} onChange={event=>setSemantic(event.target.value==="semantic")}><option value="keyword">Keywords (Tantivy)</option><option value="semantic">Semantic (BGE + LanceDB)</option></select>
      <select aria-label="Indexed search category" className="h-8 rounded-sm border bg-background px-2 text-xs" value={category} onChange={event => setCategory(event.target.value as IndexCategory)}>
        <option value="all">All indexed data</option><option value="0">Filesystem metadata</option><option value="1">Artifacts</option><option value="2">File contents</option>
      </select>
      <Button size="sm" type="submit" disabled={!database || !isTauri() || !query.trim() || loading}>{loading ? <LoaderCircle className="size-3.5 animate-spin" /> : <Search className="size-3.5" />}Search</Button>
    </form>
    <div className="flex min-h-8 shrink-0 flex-wrap items-center gap-2 border-b px-2 py-1 text-[11px] text-muted-foreground">
      <span className="min-w-0 flex-1">{!database ? "Open a case to search its index." : status?.running ? status.message : status?.message || "Search committed index data without scanning evidence files."}</span>
      <Button size="xs" variant="outline" disabled={!database || !isTauri() || requesting || status?.running} onClick={async () => {
        setRequesting(true); setNotice("");
        try { await invoke("request_case_index"); setNotice("Indexing requested. Search again after indexing to refresh results."); }
        catch (error) { setNotice(String(error)); }
        finally { setRequesting(false); }
      }}>Index now</Button>
      <Link className="text-primary underline underline-offset-2" to="/settings">Indexing settings</Link>
    </div>
    {error && <p role="alert" className="shrink-0 border-b px-2 py-1 text-xs text-destructive">{error}</p>}
    {notice && <p role="status" className="shrink-0 border-b px-2 py-1 text-xs text-muted-foreground">{notice}</p>}
    <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
      <ResizablePanel defaultSize="42%" minSize="24%">
        <section className="flex h-full min-h-0 flex-col" aria-label="Indexed results">
          <div className="flex h-8 shrink-0 items-center border-b px-2 text-xs font-medium">Results{result && result.ready && <span className="ml-2 font-normal text-muted-foreground">{result.total.toLocaleString()} {submitted?.semantic ? "ranked passages" : "matching chunks"}</span>}</div>
          <div className="min-h-0 flex-1 overflow-auto" aria-busy={loading}>
            {readableHits.map(({ hit, display }) => <button type="button" key={`${hit.group}:${hit.chunk}`} aria-pressed={selected?.group === hit.group && selected.chunk === hit.chunk} onClick={() => setSelected(hit)} className={`block w-full space-y-1 border-b px-2 py-2 text-left text-xs hover:bg-muted/40 ${selected?.group === hit.group && selected.chunk === hit.chunk ? "bg-primary/10" : ""}`}>
              <span className="block truncate font-medium">{display.title}</span>
              <span className="block truncate text-[10px] text-muted-foreground" title={hit.source}>{hit.source}</span>
              <span className="block text-[10px] text-muted-foreground">{indexedCategoryName(hit.stage)} Â· Chunk {hit.chunk + 1}</span>
              <span className="line-clamp-3 whitespace-pre-wrap break-words text-muted-foreground">{display.summary}</span>
            </button>)}
            {!result && <p className="p-3 text-xs text-muted-foreground">{semantic ? "Describe what you are looking for in natural language. Semantic search ranks related passages; use keyword search for exact identifiers." : "Search the case index for filesystem metadata, artifact rows and text content. Use quotes for a phrase, or AND, OR and NOT for advanced queries."}</p>}
            {result && !result.ready && <p className="p-3 text-xs text-muted-foreground">This case has no search index yet. Choose Index now or configure automatic indexing in Settings.</p>}
            {result?.ready && result.hits.length === 0 && <p className="p-3 text-xs text-muted-foreground">No indexed matches. Newly added data appears after indexing; excluded content can still be searched with Grep.</p>}
          </div>
          <div className="flex h-8 shrink-0 items-center justify-between gap-1 border-t px-2 text-[11px] text-muted-foreground">
            <span>{result?.hits.length ? `${offset + 1}â€“${offset + result.hits.length} of ${result.total.toLocaleString()}` : "0 results"}</span>
            <div className="flex gap-1">
              <Button size="xs" variant="ghost" aria-label="Previous indexed results" disabled={loading || offset === 0 || !submitted} onClick={() => submitted && void search(submitted.query, submitted.category, Math.max(0, offset - indexedPageSize), submitted.semantic)}><ChevronLeft className="size-3.5" /></Button>
              <Button size="xs" variant="ghost" aria-label="Next indexed results" disabled={loading || !submitted || !result || offset + indexedPageSize >= result.total || offset + indexedPageSize > indexedMaxOffset} onClick={() => submitted && void search(submitted.query, submitted.category, offset + indexedPageSize, submitted.semantic)}><ChevronRight className="size-3.5" /></Button>
            </div>
          </div>
        </section>
      </ResizablePanel>
      <ResizableHandle withHandle />
      <ResizablePanel defaultSize="58%" minSize="28%">
        <section className="flex h-full min-h-0 flex-col" aria-label="Indexed record preview">
          <div className="flex h-8 shrink-0 items-center border-b px-2 text-xs font-medium">Indexed record</div>
          {selected ? <div className="min-h-0 flex-1 overflow-auto p-3 text-xs">
            <dl className="mb-3 grid grid-cols-[4rem_minmax(0,1fr)] gap-x-2 gap-y-1 border-b pb-3">
              <dt className="text-muted-foreground">Source</dt><dd className="break-all select-text">{selected.source}</dd>
              <dt className="text-muted-foreground">Category</dt><dd>{indexedCategoryName(selected.stage)}</dd>
              <dt className="text-muted-foreground">Record</dt><dd className="break-all font-mono select-text">{selected.group}</dd>
              <dt className="text-muted-foreground">Chunk</dt><dd>{selected.chunk + 1}</dd>
            </dl>
            {selectedDisplay && <IndexedRecordPreview hit={selected} display={selectedDisplay} />}
          </div> : <p className="p-3 text-xs text-muted-foreground">Select a result to view its indexed text and source reference.</p>}
        </section>
      </ResizablePanel>
    </ResizablePanelGroup>
    <footer className="flex h-6 shrink-0 items-center gap-3 border-t px-2 text-[11px] text-muted-foreground"><span>Engine: {submitted?.semantic ? "BGE Large / LanceDB" : "Tantivy"}</span><span>Ranked by relevance</span>{submitted && <span className="truncate">Query: {submitted.query}</span>}{result && result.total > indexedMaxOffset + indexedPageSize && <span>Narrow your query to view more results.</span>}</footer>
  </div>;
}
