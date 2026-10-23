import { useState } from "react";
import { Settings } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { applyPerformanceSettings, defaultPerformanceSettings, nativeMemoryMb, recordCacheBytes, thumbnailCacheBytes, usePerformanceSettings, performancePresets, thumbnailConcurrency } from "./performanceSettings";
import { clearPreparedThumbnails, preparedThumbnails } from "../media/services/preparedThumbnails";
import { getAnalysisMode, saveAnalysisMode, type AnalysisMode } from "./performanceSettings";
import { PluginModels } from "./PluginModels";

export function SettingsPage() {
  const applied = usePerformanceSettings();
  const [draft, setDraft] = useState(applied);
  const [analysisMode, setAnalysisMode] = useState<AnalysisMode>(getAnalysisMode);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  async function save(event: React.FormEvent) {
    event.preventDefault();
    setSaving(true); setMessage("");
    try { await applyPerformanceSettings(draft); saveAnalysisMode(analysisMode); setMessage("Settings saved. Analysis mode applies to new plugin runs."); }
    catch (error) { setMessage(`Could not save settings: ${String(error)}`); }
    finally { setSaving(false); }
  }
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b px-2"><Settings className="size-3.5" /><h1 className="text-xs font-medium">Settings</h1></div>
      <div className="min-h-0 overflow-auto p-2"><div className="grid max-w-2xl gap-5">
      <form onSubmit={(event) => void save(event)} className="grid gap-3 text-xs">
        <h2 className="border-b pb-1 font-medium">Media gallery performance</h2>
        <p className="text-muted-foreground">Choose Memory saver for a low-memory laptop, or increase caching and loading speed on a more powerful system. Automatic defaults use Memory saver when system memory cannot be detected.</p>
        <div className="flex flex-wrap gap-2" aria-label="Performance presets">
          {Object.entries(performancePresets).map(([name, preset]) => (
            <Button key={name} size="sm" variant="outline" type="button" disabled={saving} onClick={() => setDraft({ ...preset })}>{name}</Button>
          ))}
        </div>
        <label className="grid grid-cols-[13rem_8rem] items-center gap-2">Managed memory budget (MiB)
          <Input aria-label="Managed memory budget (MiB)" className="h-8 rounded-sm text-xs" type="number" min={192} max={8192} step={1} required value={draft.memoryMb} onChange={(event) => setDraft({ ...draft, memoryMb: event.target.valueAsNumber })} />
        </label>
        <p className="text-muted-foreground">{nativeMemoryMb(draft)} MiB for native thumbnail workers, {Math.floor(thumbnailCacheBytes(draft) / 1048576)} MiB for decoded thumbnails, and {Math.floor(recordCacheBytes(draft) / 1048576)} MiB for gallery records. These are working estimates; browser overhead and other app features add to total RAM use.</p>
        <label className="grid grid-cols-[13rem_8rem] items-center gap-2">Screens ahead of scrolling
          <Input aria-label="Screens ahead of scrolling" className="h-8 rounded-sm text-xs" type="number" min={0} max={20} required value={draft.screensAhead} onChange={(event) => setDraft({ ...draft, screensAhead: event.target.valueAsNumber })} />
        </label>
        <label className="grid grid-cols-[13rem_8rem] items-center gap-2">Screens behind scrolling
          <Input aria-label="Screens behind scrolling" className="h-8 rounded-sm text-xs" type="number" min={0} max={10} required value={draft.screensBehind} onChange={(event) => setDraft({ ...draft, screensBehind: event.target.valueAsNumber })} />
        </label>
        <p className="text-muted-foreground">The lookahead follows your scroll direction and fits within the memory budget. Cached images can appear immediately; fast scrolling or large jumps can still need loading. Refreshing the gallery revalidates the evidence snapshot.</p>
        <p className="text-muted-foreground">Up to {thumbnailConcurrency(draft)} image previews load at once. Leaving the gallery releases its image cache. Images that exceed safe decoding limits show an unavailable preview.</p>
        <h2 className="border-b pb-1 font-medium">Media analysis performance</h2>
        <label className="grid grid-cols-[13rem_12rem] items-center gap-2">CLIP and YuNet processing
          <select aria-label="Media analysis mode" className="h-8 rounded-sm border bg-background px-2" value={analysisMode} disabled={saving} onChange={event => setAnalysisMode(event.target.value as AnalysisMode)}>
            <option value="saver">Resource saver</option>
            <option value="balanced">Balanced</option>
            <option value="performance">High performance</option>
          </select>
        </label>
        <p className="text-muted-foreground">CLIP uses your NVIDIA GPU when available; YuNet runs on CPU. High performance uses up to 16 CPU workers and batches of 64 images, Balanced up to 8 workers and batches of 32, and Resource saver up to 2 workers and batches of 8. Larger CLIP models and limited GPU memory automatically use smaller batches. Face detection resolution and confidence stay the same in every mode.</p>
        <div className="flex gap-2 border-t pt-2">
          <Button size="sm" type="submit" disabled={saving}>{saving ? "Saving..." : "Apply settings"}</Button>
          <Button size="sm" variant="outline" type="button" disabled={saving} onClick={() => { setDraft({ ...defaultPerformanceSettings }); setAnalysisMode("balanced"); }}>Restore defaults</Button>
          <Button size="sm" variant="outline" type="button" onClick={() => { const count = preparedThumbnails.size; clearPreparedThumbnails(); setMessage(`Cleared ${count} thumbnails from RAM. Disk thumbnails remain available.`); }}>Clear thumbnail RAM cache</Button>
        </div>
        <p role="status" className="min-h-4 text-muted-foreground">{message}</p>
      </form>
      <PluginModels />
      </div></div>
    </div>
  );
}
