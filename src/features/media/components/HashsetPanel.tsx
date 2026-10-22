import { useEffect, useState } from "react";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Fingerprint, FolderSearch, Upload, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { importMediaHashset, listMediaHashsets, listMediaHashsetRuns, startMediaHashsetScan, cancelMediaHashsetScan, listHashsetHitMediaPage, isHashsetScanActive, type MediaHashset, type HashsetRun, type HashsetFilter } from "../services/hashsetRepository";
import type { FileViewMediaPage } from "../services/mediaRepository";

type Props = {
  caseDatabasePath: string; datasourceId: string; indexed: boolean;
  selected: HashsetFilter | null; onSelect(filter: HashsetFilter | null): void;
};

export function HashsetPanel({ caseDatabasePath, datasourceId, indexed, selected, onSelect }: Props) {
  const [open, setOpen] = useState(false);
  const [sets, setSets] = useState<MediaHashset[]>([]);
  const [runs, setRuns] = useState<HashsetRun[]>([]);
  const [setId, setSetId] = useState("");
  const [runId, setRunId] = useState("");
  const [filePath, setFilePath] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [revision, setRevision] = useState(0);
  const [type, setType] = useState<"image" | "video">("image");
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<FileViewMediaPage>();
  const [hitsLoading, setHitsLoading] = useState(false);
  const [hitsError, setHitsError] = useState("");
  const run = runs.find(value => value.id === runId);
  const active = runs.find(isHashsetScanActive);
  const activeSet = sets.find(value => value.id === setId);

  useEffect(() => {
    if (!open) return;
    let current = true;
    setLoading(true); setError("");
    Promise.all([listMediaHashsets(caseDatabasePath), listMediaHashsetRuns(caseDatabasePath, datasourceId, indexed)]).then(([nextSets, nextRuns]) => {
      if (!current) return;
      setSets(nextSets); setRuns(nextRuns);
      setSetId(value => nextSets.some(set => set.id === value) ? value : nextRuns.find(value => value.id === selected?.runId)?.setId ?? nextSets[0]?.id ?? "");
      setRunId(value => nextRuns.some(run => run.id === value) ? value : selected?.runId ?? nextRuns[0]?.id ?? "");
    }, reason => { if (current) setError(String(reason)); }).finally(() => { if (current) setLoading(false); });
    return () => { current = false; };
  }, [open, caseDatabasePath, datasourceId, indexed, revision, selected?.runId]);

  useEffect(() => {
    if (!open || !active) return;
    let current = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await listMediaHashsetRuns(caseDatabasePath, datasourceId, indexed);
        if (current) { setRuns(next); if (next.some(isHashsetScanActive)) timer = setTimeout(poll, 1000); }
      } catch (reason) { if (current) { setError(String(reason)); timer = setTimeout(poll, 3000); } }
    };
    timer = setTimeout(poll, 500);
    return () => { current = false; clearTimeout(timer); };
  }, [open, active?.id, caseDatabasePath, datasourceId, indexed]);

  // Only display committed pages from a settled scan. Cancelling preserves partial hits.
  useEffect(() => {
    if (!open || !run || isHashsetScanActive(run)) { setPage(undefined); return; }
    let current = true;
    setHitsLoading(true); setHitsError(""); setPage(undefined);
    listHashsetHitMediaPage(caseDatabasePath, run.id, type, offset).then(next => { if (current) setPage(next); }, reason => { if (current) setHitsError(String(reason)); })
      .finally(() => { if (current) setHitsLoading(false); });
    return () => { current = false; };
  }, [open, run?.id, run?.status, caseDatabasePath, type, offset, revision]);

  async function chooseFile() {
    setError("");
    try {
      const path = await openFile({ title: "Add media hashset", multiple: false, directory: false });
      if (typeof path === "string") { setFilePath(path); setName(path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "Hashset"); }
    } catch (reason) { setError(String(reason)); }
  }
  async function importSet(event: React.FormEvent) {
    event.preventDefault(); setBusy(true); setError(""); setMessage("");
    try {
      const imported = await importMediaHashset(caseDatabasePath, filePath, name);
      setSets(current => [imported, ...current]); setSetId(imported.id); setRunId(""); setFilePath(""); setName("");
      setMessage(`Imported ${imported.hashCount.toLocaleString()} unique hashes; ${imported.duplicateCount.toLocaleString()} duplicates and ${imported.invalidLineCount.toLocaleString()} invalid lines skipped.`);
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }
  async function scan() {
    setBusy(true); setError(""); setMessage("");
    try {
      const next = await startMediaHashsetScan(caseDatabasePath, setId, datasourceId, indexed);
      setRuns(current => [next, ...current]); setRunId(next.id); setOffset(0);
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }
  async function cancel() {
    if (!active) return;
    setBusy(true); setError("");
    try { await cancelMediaHashsetScan(caseDatabasePath, active.id); setMessage("Cancellation requested. Partial hits will be saved."); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }
  return <div className="grid gap-2 border-b p-2 text-xs">
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline" className="w-full justify-start text-xs"><Fingerprint className="size-3.5" />Hashsets</Button></DialogTrigger>
      <DialogContent className="flex max-h-[90vh] flex-col sm:max-w-4xl">
        <DialogHeader><DialogTitle>Media hashsets</DialogTitle>
          <DialogDescription>Import known MD5, SHA-1, or SHA-256 hashes and scan images and videos in this datasource. Sets and hits are saved with the case.</DialogDescription></DialogHeader>
        <div className="grid min-h-0 gap-4 overflow-auto sm:grid-cols-[18rem_minmax(0,1fr)]">
          <div className="grid content-start gap-3 text-xs">
            <form className="grid gap-2 rounded-md border p-3" onSubmit={event => void importSet(event)}>
              <h3 className="font-medium">Add a hashset</h3>
              <p className="text-muted-foreground">UTF-8 text, checksum lists, or CSV with a hash column. Hash type is detected by length. Blank lines and headers are ignored.</p>
              <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => void chooseFile()}><Upload className="size-3.5" />Choose file</Button>
              {filePath && <p className="break-all text-[11px] text-muted-foreground">{filePath}</p>}
              <label className="grid gap-1">Hashset name<Input value={name} maxLength={120} required disabled={busy} onChange={event => setName(event.target.value)} className="text-xs" /></label>
              <Button size="sm" disabled={busy || !filePath || !name.trim()}>{busy ? "Working…" : "Import hashset"}</Button>
            </form>
            <div className="grid gap-2 rounded-md border p-3">
              <label className="grid gap-1 font-medium">Hashset
                <select aria-label="Hashset" className="h-8 w-full min-w-0 rounded-sm border bg-background px-2 text-xs" value={setId} disabled={busy || loading} onChange={event => {
                  setSetId(event.target.value); setRunId(runs.find(run => run.setId === event.target.value)?.id ?? ""); setOffset(0);
                }}><option value="">{loading ? "Loading hashsets…" : "Select a hashset"}</option>{sets.map(set => <option key={set.id} value={set.id}>{set.name} ({set.hashCount.toLocaleString()})</option>)}</select>
              </label>
              {activeSet && <p className="text-muted-foreground">{activeSet.hashCount.toLocaleString()} unique hashes. {activeSet.invalidLineCount.toLocaleString()} invalid lines skipped during import.</p>}
              <p className="text-muted-foreground">Scans all {indexed ? "byte-detected" : "extension-matched"} media in this datasource, across directories and classifications.</p>
              <Button size="sm" disabled={!setId || !!active || busy || loading} onClick={() => void scan()}>Scan images and videos</Button>
              {active && <Button size="sm" variant="outline" disabled={busy} onClick={() => void cancel()}>Cancel scan</Button>}
            </div>
            {message && <p role="status" className="break-words text-muted-foreground">{message}</p>}
            {error && <div className="grid gap-1"><p role="alert" className="break-words text-destructive">{error}</p><Button size="xs" variant="outline" disabled={busy} onClick={() => setRevision(value => value + 1)}>Reload</Button></div>}
          </div>
          <section className="flex min-h-0 min-w-0 flex-col rounded-md border p-3 text-xs" aria-label="Hashset hits">
            <label className="grid gap-1 font-medium">Scan results
              <select aria-label="Hashset scan results" className="h-8 min-w-0 rounded-sm border bg-background px-2 font-normal" value={runId} onChange={event => { setRunId(event.target.value); setOffset(0); }}>
                <option value="">Select a scan</option>{runs.map(value => <option key={value.id} value={value.id}>{sets.find(set => set.id === value.setId)?.name ?? "Hashset"} · {new Date(Number(value.createdAt)).toLocaleString()} · {value.status}</option>)}
              </select>
            </label>
            {run ? <>
              <p role="status" className="mt-3 font-medium">{run.status} · {run.scanned.toLocaleString()} checked · {run.hits.toLocaleString()} file hits · {run.failed.toLocaleString()} errors</p>
              {run.currentPath && <p className="mt-1 truncate text-muted-foreground" title={run.currentPath}>{run.currentPath}</p>}
              {run.error && <p role="alert" className="mt-1 break-all text-destructive">{run.error}</p>}
              {run.status !== "complete" && !isHashsetScanActive(run) && <p className="mt-1 text-muted-foreground">Partial results: this scan did not finish.</p>}
              <div className="my-2 flex gap-1">{(["image", "video"] as const).map(value => <Button key={value} size="xs" variant={type === value ? "secondary" : "ghost"} aria-pressed={type === value} onClick={() => { setType(value); setOffset(0); }}>{value === "image" ? "Images" : "Videos"}</Button>)}</div>
              <div className="h-64 overflow-auto" aria-busy={hitsLoading}>
                {isHashsetScanActive(run) ? <p className="text-muted-foreground">Scanning in the background. You can close this dialog; reopen it to check progress.</p>
                  : hitsLoading ? <p role="status">Loading hits…</p> : hitsError ? <p role="alert" className="text-destructive">{hitsError}</p>
                  : !page?.items.length ? <p className="text-muted-foreground">No {type === "image" ? "image" : "video"} hits in this scan.</p>
                  : <ul>{page.items.map(item => <li key={item.path} className="grid gap-1 border-b py-2 last:border-0">
                    <div className="flex min-w-0 items-start gap-2"><span className="min-w-0 flex-1 break-all font-medium">{item.name}</span>
                      <button className="shrink-0 p-1 hover:bg-accent" aria-label={`Reveal ${item.name}`} onClick={() => void revealItemInDir(item.mediaPath).catch(reason => setError(String(reason)))}><FolderSearch className="size-3.5" /></button></div>
                    <p className="break-all text-[11px] text-muted-foreground">{item.path}</p>
                    <p className="break-all font-mono text-[10px]">{item.metadata.find(field => field.label === "Hashset hit")?.value}</p>
                  </li>)}</ul>}
              </div>
              {page && <div className="mt-2 flex items-center justify-between gap-2 border-t pt-2 text-[11px]"><span>{page.totalCount ? `${offset + 1}–${offset + page.items.length} of ${page.totalCount.toLocaleString()}` : "0 hits"}</span>
                <div className="flex gap-1"><Button size="xs" variant="outline" disabled={offset === 0 || hitsLoading} onClick={() => setOffset(value => Math.max(0, value - 500))}>Previous</Button><Button size="xs" variant="outline" disabled={!page.hasNextPage || hitsLoading} onClick={() => setOffset(value => value + 500)}>Next</Button></div></div>}
            </> : <p className="mt-3 text-muted-foreground">Import a hashset, then scan this datasource to find matching media.</p>}
          </section>
        </div>
        <DialogFooter><Button variant="outline" onClick={() => setOpen(false)}>Close</Button>
          <Button disabled={!run || isHashsetScanActive(run) || !run.hits} onClick={() => {
            if (run) { onSelect({ runId: run.id, name: sets.find(set => set.id === run.setId)?.name ?? "Hashset" }); setOpen(false); }
          }}>Show hits in gallery</Button></DialogFooter>
      </DialogContent>
    </Dialog>
    {selected && <div className="flex min-w-0 items-start gap-1 rounded-sm bg-accent p-1.5"><span className="min-w-0 flex-1 break-words text-[11px]">Hits: {selected.name}</span>
      <button type="button" className="shrink-0 p-0.5 hover:bg-muted" aria-label="Clear hashset filter" onClick={() => onSelect(null)}><X className="size-3.5" /></button></div>}
  </div>;
}
