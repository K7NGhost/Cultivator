import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";

type Model = {
  id: string; name: string; description: string; source: string; license: string;
  ready: boolean; directory: string; downloadBytes: number; dependencies: string[];
};
type Download = {
  modelId: string; running: boolean; downloaded: number; total: number; message: string; error?: string | null;
};
type ModelSettings = { models: Model[]; download?: Download | null };
const size = (bytes: number) => bytes >= 1024 ** 3
  ? `${(bytes / 1024 ** 3).toFixed(2)} GiB`
  : bytes >= 1024 ** 2 ? `${(bytes / 1024 ** 2).toFixed(1)} MiB` : `${(bytes / 1024).toFixed(0)} KiB`;

export function PluginModels() {
  const [settings, setSettings] = useState<ModelSettings>();
  const [error, setError] = useState("");
  const [starting, setStarting] = useState(false);
  const mounted = useRef(true);
  const refresh = useCallback(async () => {
    try {
      const result = await invoke<ModelSettings>("plugin_model_settings");
      if (mounted.current) setSettings(result);
    } catch (error) { if (mounted.current) setError(String(error)); }
  }, []);
  useEffect(() => {
    mounted.current = true;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      await refresh();
      if (!stopped) timer = setTimeout(() => void poll(), 1000);
    }
    void poll();
    return () => { stopped = true; mounted.current = false; clearTimeout(timer); };
  }, [refresh]);
  async function download(id: string) {
    setStarting(true); setError("");
    try { await invoke("download_plugin_model", { modelId: id }); await refresh(); }
    catch (error) { if (mounted.current) setError(String(error)); }
    finally { if (mounted.current) setStarting(false); }
  }
  async function cancel() {
    try { await invoke("cancel_plugin_model_download"); await refresh(); }
    catch (error) { if (mounted.current) setError(String(error)); }
  }
  const active = settings?.download;
  const busy = starting || active?.running;
  return (
    <section className="grid gap-3 text-xs" aria-label="Plugin models">
      <h2 className="border-b pb-1 font-medium">Models</h2>
      <p className="text-muted-foreground">Download models before running plugins. Downloads are shared across cases and reused locally. OpenCLIP models include weights and configuration; plugins install their own Python dependencies.</p>
      {!settings && !error && <p role="status">Loading model status...</p>}
      {settings?.models.map(model => (
        <div key={model.id} className="grid gap-2 rounded-sm border p-3">
          <div className="flex items-start justify-between gap-3">
            <h3 className="font-medium">{model.name}</h3>
            <span className={model.ready ? "shrink-0 text-green-600 dark:text-green-400" : "shrink-0 text-muted-foreground"}>{model.ready ? "Downloaded" : "Not ready"}</span>
          </div>
          <p className="text-muted-foreground">{model.description}</p>
          <p className="text-muted-foreground">{size(model.downloadBytes)}{model.dependencies.length ? " including required models (already downloaded files are reused)" : ""} · {model.license}</p>
          <p className="break-all text-[11px] text-muted-foreground" title={model.directory}>{model.directory}</p>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant={model.ready ? "outline" : "default"} type="button" disabled={busy} onClick={() => void download(model.id)}>{model.ready ? "Verify / repair" : "Download"}</Button>
            <Button size="sm" variant="ghost" type="button" onClick={() => void openUrl(model.source).catch(error => setError(String(error)))}>Model source</Button>
          </div>
        </div>
      ))}
      {active && <div className="grid gap-2 rounded-sm border p-3" role="status" aria-live="polite">
        <p>{settings?.models.find(model => model.id === active.modelId)?.name}: {active.message}</p>
        {active.running && <>
          <progress className="h-2 w-full" max={Math.max(1, active.total)} value={active.downloaded} aria-label="Model download progress" />
          <div className="flex items-center justify-between gap-2"><span>{size(active.downloaded)} / {size(active.total)}</span>
            <Button size="sm" variant="outline" type="button" onClick={() => void cancel()}>Cancel download</Button></div>
        </>}
      </div>}
      {error && <p role="alert" className="text-destructive">{error}</p>}
      <p className="text-muted-foreground">Cancelled or interrupted downloads can be resumed. Files are checked with SHA-256 before they become available to plugins.</p>
    </section>
  );
}
