import { useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useTimeZone } from "@/features/settings/timeZoneSettings";
import { artifactModelsByKind } from "../model/artifactModels";
import type { ArtifactModelDefinition, CustomTableColumn, StoredArtifactRecord } from "../model/types";
import { formatArtifactTableCell } from "../utils/artifactTableFormatting";
import { ArtifactDataPreview } from "./ArtifactDataPreview";
import { ArtifactRowContextMenu, ArtifactRowTagButton } from "./ArtifactRowTagMenu";
import { ArtifactRowTagDialog } from "./ArtifactRowTagDialog";
import { useArtifactRowTags } from "../hooks/useArtifactRowTags";
import { subscribeArtifactTagChanges } from "../services/artifactTagRepository";

type ViewRow = { key: string; values: Record<string, unknown>; occurrenceCount: number; primarySourceIndex: number };
type Page = { viewKey: string; name: string; columns: CustomTableColumn[]; rows: ViewRow[]; total: number; totalOccurrences: number;
  offset: number; nextOffset: number; shortened: boolean };
type PayloadPage = { text: string; total: number; offset: number; nextOffset: number };
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const models: ReadonlyMap<string, ArtifactModelDefinition> = artifactModelsByKind;

function PayloadViewer({ databasePath, artifact, pluginName, datasourceName }: {
  databasePath: string; artifact: StoredArtifactRecord; pluginName?: string; datasourceName?: string;
}) {
  const timeZone = useTimeZone();
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<PayloadPage | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false; setPage(null); setError("");
    void invoke<PayloadPage>("artifact_payload_page", { caseDatabasePath: databasePath, artifactId: artifact.id, offset })
      .then((value) => { if (!disposed) setPage(value); }).catch((error: unknown) => { if (!disposed) setError(errorText(error)); });
    return () => { disposed = true; };
  }, [databasePath, artifact.id, offset]);
  return <div className="flex min-h-0 flex-1 flex-col">
    <div className="shrink-0 space-y-1 border-b p-2 text-xs select-text">
      <div>{artifact.label} · {artifact.resultKind}</div><div className="break-all">{artifact.filePath}</div>
      <div className="break-all">Plugin: {pluginName ?? artifact.pluginId} · Datasource: {datasourceName ?? artifact.datasourceId}</div>
      <div>Created: {formatArtifactTableCell(artifact.createdAt, { key: "createdAt", label: "Created", format: "datetime", timestampUnit: "milliseconds" }, timeZone)}</div>
      <div className="break-all">Job: {artifact.jobId}</div>
      <div>Original JSON{page ? ` · characters ${(page.offset + 1).toLocaleString()}–${page.nextOffset.toLocaleString()} of ${page.total.toLocaleString()}` : ""}</div>
    </div>
    {error && <p role="alert" className="p-2 text-xs text-destructive">{error}</p>}
    <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-all p-2 text-[11px] select-text">{page?.text ?? (!error ? "Loading original JSON…" : "")}</pre>
    <div className="flex shrink-0 justify-end gap-2 border-t p-2">
      <Button size="sm" variant="outline" disabled={!page || offset === 0} onClick={() => setOffset(Math.max(0, offset - 32768))}>Previous JSON page</Button>
      <Button size="sm" variant="outline" disabled={!page || page.nextOffset >= page.total} onClick={() => setOffset(page!.nextOffset)}>Next JSON page</Button>
    </div>
  </div>;
}

function RowSources({ databasePath, viewKey, row, sources, onSelect }: {
  databasePath: string; viewKey: string; row: ViewRow; sources: StoredArtifactRecord[]; onSelect: (artifact: StoredArtifactRecord) => void;
}) {
  const [offset, setOffset] = useState(0);
  const [result, setResult] = useState<{ total: number; items: { sourceIndex: number; count: number }[] } | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false; setResult(null);
    void invoke<typeof result>("artifact_row_sources", { caseDatabasePath: databasePath, viewKey, rowKey: row.key, offset })
      .then((value) => { if (!disposed) setResult(value); }).catch((error: unknown) => { if (!disposed) setError(errorText(error)); });
    return () => { disposed = true; };
  }, [databasePath, viewKey, row.key, offset]);
  return <div className="max-h-52 shrink-0 overflow-auto border-t p-2 text-xs" aria-label="Row sources">
    <p>{row.occurrenceCount.toLocaleString()} occurrences{result ? ` · ${result.total.toLocaleString()} sources` : ""}</p>
    {error && <p role="alert">{error}</p>}
    {result?.items.map((item) => {
      const artifact = sources[item.sourceIndex];
      return artifact && <button key={item.sourceIndex} className="block w-full truncate py-1 text-left underline" title={artifact.filePath}
        onClick={() => onSelect(artifact)}>{artifact.filePath || artifact.label} · {item.count} occurrences</button>;
    })}
    {result && result.total > 100 && <div className="flex gap-2">
      <Button size="sm" variant="outline" disabled={!offset} onClick={() => setOffset(offset - 100)}>Previous sources</Button>
      <Button size="sm" variant="outline" disabled={offset + 100 >= result.total} onClick={() => setOffset(offset + 100)}>Next sources</Button>
    </div>}
  </div>;
}

export function PagedArtifactPanel({ databasePath, revision, artifact, artifacts, onSelect, pluginName, datasourceName, taggedOnly = false }: {
  databasePath: string; revision: string; artifact: StoredArtifactRecord; artifacts: StoredArtifactRecord[];
  onSelect: (artifact: StoredArtifactRecord) => void;
  pluginName?: string; datasourceName?: string;
  taggedOnly?: boolean;
}) {
  const timeZone = useTimeZone();
  const [mode, setMode] = useState<"all" | "selected">("all");
  const [tab, setTab] = useState("table");
  const [sourceQuery, setSourceQuery] = useState("");
  const [sourceOffset, setSourceOffset] = useState(0);
  const [offset, setOffset] = useState(0);
  const [previousOffsets, setPreviousOffsets] = useState<number[]>([]);
  const [sort, setSort] = useState({ key: "", descending: false });
  const [page, setPage] = useState<Page | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [sourceRow, setSourceRow] = useState<ViewRow | null>(null);
  const [selectedRowKey, setSelectedRowKey] = useState("");
  const [retry, setRetry] = useState(0);
  const [tagTarget, setTagTarget] = useState<{ row: ViewRow; viewKey: string } | null>(null);
  const rowTags = useArtifactRowTags(databasePath, page?.viewKey ?? "", page?.rows.map((row) => row.key) ?? [], !loading && tab === "table" && !!page);
  const isMap = artifact.resultKind === "map_layer";
  const selected = useMemo(() => mode === "all" ? artifacts : [artifact], [artifacts, artifact, mode]);
  const ids = useMemo(() => selected.map((item) => item.id), [selected]);
  const idsKey = JSON.stringify(ids);
  useEffect(() => { setTagTarget(null); }, [databasePath, idsKey, revision]);
  useEffect(() => subscribeArtifactTagChanges(databasePath, () => {
    if (taggedOnly) { setOffset(0); setPreviousOffsets([]); setRetry((value) => value + 1); }
  }), [databasePath, taggedOnly]);
  const requestVersion = useRef(0);
  const modelColumns = useMemo(() => (models.get(artifact.resultKind)?.fields ?? []).map((field) => ({
    key: field.key, label: field.label, format: field.type === "datetime" ? "datetime" : undefined,
  })), [artifact.resultKind]);
  useEffect(() => {
    setOffset(0); setPreviousOffsets([]); setSourceRow(null);
  }, [idsKey, revision, taggedOnly]);
  useEffect(() => {
    if (isMap || tab === "properties") return;
    const version = ++requestVersion.current;
    const requestId = crypto.randomUUID();
    let disposed = false; setPage(null); setError(""); setLoading(true); setSourceRow(null);
    let unlisten: UnlistenFn | undefined;
    const ready = listen<{ requestId: string; page: Page }>("artifact-view-preview", (event) => {
      if (!disposed && requestVersion.current === version && event.payload.requestId === requestId) setPage(event.payload.page);
    }).then((off) => { if (disposed) off(); else unlisten = off; }).catch(() => undefined);
    // Avoid indexing intermediate selections during quick keyboard navigation.
    const timer = setTimeout(() => {
      void ready.then(() => {
        if (disposed) return;
        return invoke<Page>("artifact_view_page", { caseDatabasePath: databasePath,
          request: { requestId, artifactIds: ids, revision, offset, limit: 100, sortKey: sort.key || null, descending: sort.descending, modelColumns, taggedOnly } });
      })
        .then((value) => { if (value && !disposed && requestVersion.current === version) setPage(value); })
        .catch((error: unknown) => { if (!disposed && requestVersion.current === version) setError(errorText(error)); })
        .finally(() => { if (!disposed && requestVersion.current === version) setLoading(false); });
    }, 120);
    return () => { disposed = true; clearTimeout(timer); unlisten?.(); void invoke("cancel_artifact_view").catch(() => undefined); };
  }, [databasePath, idsKey, revision, offset, sort.key, sort.descending, modelColumns, isMap, tab, retry, taggedOnly]);
  const matchingSources = useMemo(() => artifacts.filter((item) => !sourceQuery || `${item.filePath} ${item.label}`.toLocaleLowerCase().includes(sourceQuery.toLocaleLowerCase())), [artifacts, sourceQuery]);
  const previewTable = useMemo(() => page ? { name: page.name, columns: page.columns, totalOccurrences: page.rows.reduce((sum, row) => sum + row.occurrenceCount, 0),
    deduplication: { mode: "group" as const }, sources: selected.map((item) => ({ artifactId: item.id, filePath: item.filePath })),
    rows: page.rows.map((row) => ({ values: row.values, occurrenceCount: row.occurrenceCount, primarySourceIndex: row.primarySourceIndex })) } : null, [page, selected]);
  return <section className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden" aria-label="Paged artifact viewer">
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b p-2 text-xs">
      <strong className="min-w-0 flex-1 truncate">{artifact.label || artifact.resultKind}</strong>
      <span className="text-muted-foreground">{timeZone}</span>
      <Tabs value={tab} onValueChange={setTab}><TabsList className="h-7"><TabsTrigger value="table">Table</TabsTrigger><TabsTrigger value="preview">Preview</TabsTrigger><TabsTrigger value="properties">Properties</TabsTrigger></TabsList></Tabs>
    </div>
    {artifacts.length > 1 && <details className="shrink-0 border-b p-2 text-xs">
      <summary className="cursor-pointer">{mode === "all" ? `All ${artifacts.length.toLocaleString()} source files` : artifact.filePath || artifact.label}</summary>
      <div className="mt-2 flex gap-2"><Button size="sm" variant={mode === "all" ? "secondary" : "outline"} onClick={() => setMode("all")}>All entries</Button>
        <input aria-label="Find artifact source" className="input input-xs min-w-0 flex-1" placeholder="Find a source file…" value={sourceQuery} onChange={(event) => { setSourceQuery(event.target.value); setSourceOffset(0); }} /></div>
      <div className="max-h-48 overflow-auto">{matchingSources.slice(sourceOffset, sourceOffset + 100).map((item) => <button key={item.id} className="block w-full truncate py-1 text-left hover:underline" title={item.filePath}
        onClick={() => { setMode("selected"); onSelect(item); }}>{item.filePath || item.label}</button>)}</div>
      {matchingSources.length > 100 && <div className="flex gap-2"><Button size="sm" disabled={!sourceOffset} onClick={() => setSourceOffset(sourceOffset - 100)}>Previous files</Button>
        <Button size="sm" disabled={sourceOffset + 100 >= matchingSources.length} onClick={() => setSourceOffset(sourceOffset + 100)}>Next files</Button></div>}
    </details>}
    {tab === "properties" ? <PayloadViewer key={artifact.id} databasePath={databasePath} artifact={artifact} pluginName={pluginName} datasourceName={datasourceName} />
      : isMap ? <div className="grid flex-1 place-items-center text-xs"><Link className="underline" to="/map">Open this layer in Map</Link></div>
      : error ? <div role="alert" className="p-3 text-xs text-destructive">{error}<Button className="ml-2" size="sm" onClick={() => setRetry(retry + 1)}>Retry</Button></div>
      : !page ? <div role="status" className="grid min-h-0 flex-1 place-items-center text-xs text-muted-foreground">Loading artifact rows…</div>
      : <>
        <div className="shrink-0 border-b px-2 py-1 text-[11px] text-muted-foreground">{loading ? <span role="status">Showing the first entries while the remaining entries load…</span> : <>{page.total.toLocaleString()} entries · {page.totalOccurrences.toLocaleString()} occurrences</>}
          {taggedOnly && <span> · Tagged rows only</span>}
          {page.shortened && <span> · Long values are shortened; open Properties to inspect the original JSON.</span>}
        </div>
        {rowTags.error && <div role="alert" className="shrink-0 border-b p-2 text-xs text-destructive">{rowTags.error}<Button className="ml-2 h-6 text-xs" variant="outline" size="sm" disabled={rowTags.busy} onClick={rowTags.refresh}>Reload tags</Button></div>}
        {tab === "preview" ? previewTable && <ArtifactDataPreview table={previewTable} sourceArtifacts={selected} pageOnly selectedRowIndex={page.rows.findIndex((row) => row.key === selectedRowKey)} onSelectRow={(index) => setSelectedRowKey(page.rows[index]?.key ?? "")} fallback={<div className="min-h-0 flex-1 overflow-auto p-3 text-xs"><p className="mb-3 text-muted-foreground">Selected entry on this page</p>
      <dl className="space-y-2">{page.columns.map((column) => { const row=page.rows.find((row) => row.key === selectedRowKey) ?? page.rows[0];
        return row && <div key={column.key}><dt className="font-medium text-muted-foreground">{column.label}</dt><dd className="whitespace-pre-wrap break-words select-text">{formatArtifactTableCell(row.values[column.key],column,timeZone) || "–"}</dd></div>;
      })}</dl></div>} />
        : <div className="min-h-0 flex-1 overflow-auto"><table className="min-w-full border-collapse text-left text-xs" aria-label="Artifact entries">
          <thead className="sticky top-0 bg-background"><tr>{page.columns.map((column) => <th key={column.key} className="max-w-72 border-b p-2" aria-sort={!loading && sort.key === column.key ? sort.descending ? "descending" : "ascending" : "none"}>
            <button type="button" disabled={loading} onClick={() => { setSort({ key: column.key, descending: sort.key === column.key && !sort.descending }); setOffset(0); setPreviousOffsets([]); }}>{column.label}{!loading && sort.key === column.key ? sort.descending ? " ↓" : " ↑" : ""}</button>
          </th>)}<th className="border-b p-2" title="Right-click a row or use its tag button">Row tags</th><th className="border-b p-2">Sources</th></tr></thead>
          <tbody>{page.rows.map((row) => {
            const tags = rowTags.tags[row.key] ?? [];
            const actions = { tags, availableTags: rowTags.availableTags, occurrences: row.occurrenceCount, disabled: loading || rowTags.busy || !rowTags.ready,
              onSelect: () => setSelectedRowKey(row.key), onCustom: () => setTagTarget({ row, viewKey: page.viewKey }),
              onToggle: (name: string, assigned: boolean) => { void rowTags.assign(row.key, name, assigned).catch(() => undefined); } };
            return <ArtifactRowContextMenu key={row.key} {...actions}><tr aria-selected={selectedRowKey === row.key} onClick={() => setSelectedRowKey(row.key)} onContextMenu={() => setSelectedRowKey(row.key)} className={`border-b hover:bg-muted/30 ${selectedRowKey === row.key ? "bg-primary/10" : ""}`}>{page.columns.map((column) => <td key={column.key} className="max-w-72 truncate p-2 align-top select-text">
            {formatArtifactTableCell(row.values[column.key], column, timeZone) || "–"}</td>)}
            <td className="p-2 align-top"><div className="flex items-start gap-1"><div className="flex max-w-48 flex-wrap gap-1">{tags.map((tag) => <span key={tag.name} className="max-w-40 truncate rounded-sm border bg-primary/10 px-1.5 py-0.5 text-[10px]" title={`${tag.name} · ${tag.occurrences} of ${row.occurrenceCount} occurrences`}>{tag.name}{tag.occurrences < row.occurrenceCount ? ` (${tag.occurrences}/${row.occurrenceCount})` : ""}</span>)}</div><ArtifactRowTagButton {...actions} /></div></td>
            <td className="p-2 align-top"><button className="whitespace-nowrap underline" disabled={loading} onClick={() => setSourceRow((current) => current?.key === row.key ? null : row)}>{loading ? "Loading sources…" : `${row.occurrenceCount.toLocaleString()} occurrences`}</button></td>
          </tr></ArtifactRowContextMenu>;
          })}</tbody></table>{!page.rows.length && <p className="p-3 text-xs text-muted-foreground">{taggedOnly ? "No tagged entries in this artifact." : "No entries in this artifact."}</p>}</div>}
        {sourceRow && <RowSources key={sourceRow.key} databasePath={databasePath} viewKey={page.viewKey} row={sourceRow} sources={selected} onSelect={(item) => { onSelect(item); setTab("properties"); }} />}
        <div className="flex shrink-0 items-center justify-between gap-2 border-t p-2 text-xs">
          <span>{loading ? `${page.rows.length} entries shown · loading remaining entries` : page.total ? `${(page.offset + 1).toLocaleString()}–${page.nextOffset.toLocaleString()} of ${page.total.toLocaleString()}` : "0 entries"}</span>
          <div className="flex gap-2"><Button variant="outline" size="sm" disabled={loading || !previousOffsets.length} onClick={() => { setOffset(previousOffsets[previousOffsets.length - 1]); setPreviousOffsets(previousOffsets.slice(0, -1)); }}>Previous entries</Button>
            <Button variant="outline" size="sm" disabled={loading || page.nextOffset >= page.total} onClick={() => { setPreviousOffsets([...previousOffsets, offset]); setOffset(page.nextOffset); }}>Next entries</Button></div>
        </div>
      </>}
    {tagTarget && <ArtifactRowTagDialog occurrences={tagTarget.row.occurrenceCount} availableTags={rowTags.availableTags} busy={rowTags.busy} onClose={() => setTagTarget(null)} onSave={async (name) => {
      if (tagTarget.viewKey !== page?.viewKey) throw new Error("The artifact view changed. Select its row again.");
      await rowTags.assign(tagTarget.row.key, name, true); setTagTarget(null);
    }} />}
  </section>;
}
