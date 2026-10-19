import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { ClassificationDefinition } from "../model/vicClassification";

export type CustomClassificationActions = {
  categories: ClassificationDefinition[];
  disabled: boolean;
  onSave: (category: number | null, name: string, color: string) => Promise<void>;
  onDelete: (category: number) => Promise<void>;
};

export function CustomClassificationManager({ categories, disabled, onSave, onDelete }: CustomClassificationActions) {
  const [editing, setEditing] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [color, setColor] = useState("#14b8a6");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const reset = () => { setEditing(null); setName(""); setColor("#14b8a6"); };
  async function save(event: React.FormEvent) {
    event.preventDefault(); setBusy(true); setMessage("");
    try { await onSave(editing, name.trim(), color); reset(); setMessage("Classification saved."); }
    catch (error) { setMessage(String(error)); }
    finally { setBusy(false); }
  }
  async function remove(category: number) {
    setBusy(true); setMessage("");
    try { await onDelete(category); if (editing === category) reset(); setMessage("Classification deleted. Assigned images are now unclassified."); }
    catch (error) { setMessage(String(error)); }
    finally { setBusy(false); }
  }
  return (
    <details className="border-t pt-2 text-[11px]">
      <summary className="cursor-pointer py-1 font-medium">Manage custom classifications</summary>
      <div className="grid gap-2 pt-1">
        {categories.map((entry) => <div key={entry.value} className="flex min-w-0 items-center gap-1">
          <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: entry.color }} />
          <span className="min-w-0 flex-1 truncate" title={entry.label}>{entry.label}</span>
          <button type="button" className="px-1 underline disabled:opacity-50" disabled={disabled || busy}
            aria-label={`Edit ${entry.label}`} onClick={() => { setEditing(entry.value); setName(entry.label); setColor(entry.color); setMessage(""); }}>Edit</button>
          <button type="button" className="px-1 text-destructive underline disabled:opacity-50" disabled={disabled || busy}
            aria-label={`Delete ${entry.label}`} onClick={() => { void remove(entry.value); }}>Delete</button>
        </div>)}
        <form onSubmit={(event) => { void save(event); }} className="grid gap-1.5">
          <label className="grid gap-1">Name
            <Input aria-label="Custom classification name" className="h-7 rounded-sm text-xs" maxLength={80} required
              disabled={disabled || busy} value={name} onChange={(event) => setName(event.target.value)} />
          </label>
          <label className="flex items-center justify-between">Color
            <input aria-label="Custom classification color" type="color" className="h-7 w-12 cursor-pointer rounded-sm border bg-background p-0.5"
              disabled={disabled || busy} value={color} onChange={(event) => setColor(event.target.value)} />
          </label>
          <div className="flex flex-wrap gap-1">
            <Button type="submit" size="xs" disabled={disabled || busy || !name.trim()}>{editing === null ? "Add classification" : "Save changes"}</Button>
            {editing !== null && <Button type="button" size="xs" variant="ghost" disabled={busy} onClick={reset}>Cancel</Button>}
          </div>
        </form>
        <p className="text-muted-foreground">Custom classifications are saved with this case. Deleting one clears it from assigned images.</p>
        <p role="status" className="text-muted-foreground">{message}</p>
      </div>
    </details>
  );
}
