import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useLocation } from "react-router-dom";
import { useCases } from "@/features/cases";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { loadIndexingOptions, saveIndexingOptions, type IndexingMode, type IndexingStatus } from "./indexingSettings";
import { indexingProgress } from "./indexingProgress";

export function IndexingPreferences() {
  const { activeCase } = useCases();
  const location = useLocation();
  const [options, setOptions] = useState(loadIndexingOptions);
  const [status, setStatus] = useState<IndexingStatus | null>(null);
  const [message, setMessage] = useState("");
  const [requesting, setRequesting] = useState(false);
  const [vectors,setVectors]=useState<{database?:string;message:string;processed?:number;total?:number;running?:boolean;device?:string;deviceName?:string}>();
  useEffect(() => {
    if (location.pathname !== "/settings" || !isTauri()) return;
    let disposed = false;
    const poll = async () => {
      try { const next = await invoke<IndexingStatus>("case_indexer_status"); if (!disposed) setStatus(next); const vector=await invoke<typeof vectors>("vector_index_status"); if (!disposed) setVectors(vector); }
      catch (error) { if (!disposed) setMessage(String(error)); }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 1000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [location.pathname, activeCase?.databasePath]);
  const current = status?.database === activeCase?.databasePath ? status : null;
  const currentVectors=vectors?.database===activeCase?.databasePath ? vectors : undefined;
  return <section className="grid gap-3 text-xs" aria-label="Search indexing">
    <h2 className="border-b pb-1 font-medium">Search indexing</h2>
    <form className="grid gap-3" onSubmit={async event => {
      event.preventDefault();
      try {
        if (isTauri()) await invoke("configure_case_indexer", { database: activeCase?.databasePath ?? null, options });
        saveIndexingOptions(options); setMessage("Indexing settings saved.");
      } catch (error) { setMessage(String(error)); }
    }}>
      <label className="grid grid-cols-[13rem_minmax(0,1fr)] items-center gap-2">Indexing behavior
        <select className="h-8 min-w-0 rounded-sm border bg-background px-2" value={options.mode} onChange={event => setOptions({ ...options, mode: event.target.value as IndexingMode })}>
          <option value="adaptive">Adaptive â€” Reduce activity during investigations</option>
          <option value="background">Background â€” Continuous indexing</option>
          <option value="manual">Manual â€” Index only when requested</option>
        </select>
      </label>
      <label className="grid grid-cols-[13rem_8rem] items-center gap-2">Check for additions (seconds)
        <Input type="number" className="h-8 rounded-sm text-xs" min={10} max={3600} required value={options.scanSeconds} onChange={event => setOptions({ ...options, scanSeconds: event.target.valueAsNumber })} />
      </label>
      <label className="grid grid-cols-[13rem_8rem] items-center gap-2">Maximum text file size (MiB)
        <Input type="number" className="h-8 rounded-sm text-xs" min={1} max={32} required value={options.maxFileMib} onChange={event => setOptions({ ...options, maxFileMib: event.target.valueAsNumber })} />
      </label>
      <p className="text-muted-foreground">A single background worker checks the open case for additions and changes. Adaptive waits while you interact with Cultivator. Index now runs one pass immediately, including in Manual mode. Settings apply to all cases; indexes are stored separately beside each case database.</p>
      <label className="flex items-center gap-2"><input type="checkbox" checked={options.semantic ?? false} onChange={event=>setOptions({...options,semantic:event.target.checked})} />Semantic indexing (BGE Large + LanceDB)</label>
      <p className="text-muted-foreground">Download BGE Large under Models first. First use installs a dedicated Python environment with the required dependencies. Uses GPU when available, otherwise CPU. Follows the indexing behavior above and reuses existing embeddings.</p>
      <div className="flex gap-2">
        <Button size="sm" type="submit">Apply indexing settings</Button>
        <Button size="sm" variant="outline" type="button" disabled={!activeCase || !isTauri() || requesting || current?.running} onClick={async () => {
          setRequesting(true);
          try { await invoke("request_case_index"); setMessage("Indexing requested."); }
          catch (error) { setMessage(String(error)); }
          finally { setRequesting(false); }
        }}>Index now</Button>
      </div>
    </form>
    {options.semantic && <div className="grid gap-1 text-muted-foreground" aria-busy={currentVectors?.running}>
      <span>{currentVectors ? currentVectors.message : "Semantic vectors are stored separately for each case."}</span>
      <div className="flex justify-between gap-2">
        <span>{currentVectors?.deviceName ? `${currentVectors.device === "cuda" ? "GPU: " : ""}${currentVectors.deviceName}` : "Preparing embedding runtime"}</span>
        <span>{(currentVectors?.processed ?? 0).toLocaleString()} / {(currentVectors?.total ?? 0).toLocaleString()} records checked</span>
      </div>
      <progress aria-label="Semantic indexing progress" className="h-1.5 w-full accent-primary" max={Math.max(1,currentVectors?.total??0)} value={currentVectors?.running && !currentVectors.total ? undefined : currentVectors?.processed??0} />
    </div>}
    <div className="grid gap-2" aria-label="Indexing progress">
      {(current?.stages ?? ["Filesystem metadata", "Artifacts", "File contents"].map(name => ({ name, processed: 0, total: 0, skipped: 0, state: "Waiting" }))).map(stage => {
        const progress = indexingProgress(stage);
        return <div key={stage.name} className="grid gap-1">
          <div className="flex justify-between gap-2"><span>{stage.name}</span><span className="text-muted-foreground">{progress.summary}</span></div>
          <progress aria-label={`${stage.name} completeness`} className="h-1.5 w-full accent-primary" max={100} value={progress.value} />
        </div>;
      })}
    </div>
    <p className="text-muted-foreground">{activeCase ? current?.message || "Waiting for indexer status." : "Open a case to index its data sources."}</p>
    <p className="text-muted-foreground">Saved indexes load with each case. Background checks keep the last completed coverage visible and only replace changed records. Completeness includes exclusions. Binary or unreadable files, files above the size limit, and artifact rows above 256 KiB are excluded from text indexing. Text supports UTF-8 and UTF-16 with a BOM; PDF, Office and OCR extraction are not included. Source paths, artifact row identities and overlapping text chunks are retained for future retrieval.</p>
    {message && <p role="status" className="text-muted-foreground">{message}</p>}
  </section>;
}
