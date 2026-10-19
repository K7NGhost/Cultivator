import { useState } from "react";
import { Settings } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { applyPerformanceSettings, defaultPerformanceSettings, nativeMemoryMb, recordCacheBytes, thumbnailCacheBytes, usePerformanceSettings } from "./performanceSettings";
import { clearPreparedThumbnails, preparedThumbnails } from "../media/services/preparedThumbnails";

export function SettingsPage() {
  const applied = usePerformanceSettings();
  const [draft, setDraft] = useState(applied);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  async function save(event: React.FormEvent) {
    event.preventDefault();
    setSaving(true); setMessage("");
    try { await applyPerformanceSettings(draft); setMessage("Settings saved. Running thumbnail jobs finish with their previous budget."); }
    catch (error) { setMessage(`Could not save settings: ${String(error)}`); }
    finally { setSaving(false); }
  }
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b px-2"><Settings className="size-3.5" /><h1 className="text-xs font-medium">Settings</h1></div>
      <form onSubmit={(event) => void save(event)} className="grid max-w-2xl gap-3 overflow-auto p-2 text-xs">
        <h2 className="border-b pb-1 font-medium">Media gallery performance</h2>
        <p className="text-muted-foreground">Prepare thumbnails beyond the viewport and retain recently visited images in RAM. Tiles remain virtualized so large galleries stay responsive.</p>
        <label className="grid grid-cols-[13rem_8rem] items-center gap-2">Managed memory budget (MiB)
          <Input aria-label="Managed memory budget (MiB)" className="h-8 rounded-sm text-xs" type="number" min={512} max={8192} step={1} required value={draft.memoryMb} onChange={(event) => setDraft({ ...draft, memoryMb: event.target.valueAsNumber })} />
        </label>
        <p className="text-muted-foreground">{nativeMemoryMb(draft)} MiB for native thumbnail workers, {Math.floor(thumbnailCacheBytes(draft) / 1048576)} MiB for decoded thumbnails, and {Math.floor(recordCacheBytes(draft) / 1048576)} MiB for gallery records. These are working estimates; browser overhead and other app features add to total RAM use.</p>
        <label className="grid grid-cols-[13rem_8rem] items-center gap-2">Screens ahead of scrolling
          <Input aria-label="Screens ahead of scrolling" className="h-8 rounded-sm text-xs" type="number" min={1} max={20} required value={draft.screensAhead} onChange={(event) => setDraft({ ...draft, screensAhead: event.target.valueAsNumber })} />
        </label>
        <label className="grid grid-cols-[13rem_8rem] items-center gap-2">Screens behind scrolling
          <Input aria-label="Screens behind scrolling" className="h-8 rounded-sm text-xs" type="number" min={1} max={10} required value={draft.screensBehind} onChange={(event) => setDraft({ ...draft, screensBehind: event.target.valueAsNumber })} />
        </label>
        <p className="text-muted-foreground">The lookahead follows your scroll direction and fits within the memory budget. Cached images can appear immediately; fast scrolling or large jumps can still need loading. Refreshing the gallery revalidates the evidence snapshot.</p>
        <div className="flex gap-2 border-t pt-2">
          <Button size="sm" type="submit" disabled={saving}>{saving ? "Saving..." : "Apply settings"}</Button>
          <Button size="sm" variant="outline" type="button" disabled={saving} onClick={() => setDraft({ ...defaultPerformanceSettings })}>Restore defaults</Button>
          <Button size="sm" variant="outline" type="button" onClick={() => { const count = preparedThumbnails.size; clearPreparedThumbnails(); setMessage(`Cleared ${count} thumbnails from RAM. Disk thumbnails remain available.`); }}>Clear thumbnail RAM cache</Button>
        </div>
        <p role="status" className="min-h-4 text-muted-foreground">{message}</p>
      </form>
    </div>
  );
}
