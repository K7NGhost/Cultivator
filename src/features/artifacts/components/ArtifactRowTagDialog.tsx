import { useId, useState } from "react";
import { LoaderCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

export function ArtifactRowTagDialog({ occurrences, availableTags, busy, onClose, onSave }: {
  occurrences: number; availableTags: string[]; busy: boolean; onClose: () => void; onSave: (name: string) => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  const suggestions = useId();
  return <Dialog open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
    <DialogContent className="sm:max-w-sm" showCloseButton={!busy} onEscapeKeyDown={(event) => { if (busy) event.preventDefault(); }} onInteractOutside={(event) => { if (busy) event.preventDefault(); }}>
      <DialogHeader><DialogTitle className="text-sm">Tag artifact row</DialogTitle><DialogDescription className="text-xs">{occurrences > 1 ? `Applies to all ${occurrences} occurrences represented by this row.` : "Save a tag on this artifact row."}</DialogDescription></DialogHeader>
      <form className="space-y-3" onSubmit={(event) => {
        event.preventDefault(); if (!name.trim() || busy) return;
        setError(""); void onSave(name).catch((error: unknown) => setError(error instanceof Error ? error.message : String(error)));
      }}>
        <label className="block space-y-1 text-xs">Tag name<Input aria-label="Artifact tag name" className="h-8 text-xs" autoFocus list={suggestions} value={name} maxLength={80} required disabled={busy} onChange={(event) => setName(event.target.value)} /></label>
        <datalist id={suggestions}>{availableTags.map((tag) => <option key={tag} value={tag} />)}</datalist>
        {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
        <DialogFooter><Button type="button" variant="outline" size="sm" disabled={busy} onClick={onClose}>Cancel</Button>
          <Button type="submit" size="sm" disabled={busy || !name.trim()}>{busy && <LoaderCircle className="size-3.5 animate-spin" />}Save tag</Button></DialogFooter>
      </form>
    </DialogContent>
  </Dialog>;
}
