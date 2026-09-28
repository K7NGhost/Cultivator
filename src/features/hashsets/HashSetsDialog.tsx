import { useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { ShieldAlert, ShieldCheck, Trash2, Upload } from "lucide-react";
import { toast } from "react-toastify";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  HASH_SETS_CHANGED,
  deleteHashSet,
  importHashSet,
  listHashSets,
  type HashSetInfo,
  type HashSetKind,
} from "@/features/hashsets/hashSetRepository";

/** Import and remove the hash sets every case is checked against. */
export function HashSetsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [sets, setSets] = useState<HashSetInfo[]>([]);
  const [sourcePath, setSourcePath] = useState("");
  const [name, setName] = useState("");
  const [kind, setKind] = useState<HashSetKind>("known-bad");
  const [isWorking, setIsWorking] = useState(false);

  function refresh() {
    listHashSets()
      .then(setSets)
      .catch((error) => toast.error(errorText(error)));
  }

  useEffect(() => {
    if (open) {
      refresh();
    }
  }, [open]);

  async function chooseFile() {
    const picked = await openFileDialog({ multiple: false, directory: false });
    if (typeof picked === "string") {
      setSourcePath(picked);
      if (!name.trim()) {
        setName(picked.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "");
      }
    }
  }

  async function runImport() {
    setIsWorking(true);
    try {
      const imported = await importHashSet(sourcePath, name, kind);
      toast.success(`Imported ${imported.entryCount.toLocaleString()} hashes into "${imported.name}".`);
      setSourcePath("");
      setName("");
      refresh();
      window.dispatchEvent(new Event(HASH_SETS_CHANGED));
    } catch (error) {
      toast.error(errorText(error));
    } finally {
      setIsWorking(false);
    }
  }

  async function remove(set: HashSetInfo) {
    setIsWorking(true);
    try {
      await deleteHashSet(set.id);
      refresh();
      window.dispatchEvent(new Event(HASH_SETS_CHANGED));
    } catch (error) {
      toast.error(errorText(error));
    } finally {
      setIsWorking(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(40rem,calc(100vw-2rem))] rounded-sm p-0">
        <DialogHeader className="border-b px-3 py-2">
          <DialogTitle className="text-sm">Hash sets</DialogTitle>
          <DialogDescription className="text-xs">
            Lists of known file hashes (MD5, SHA-1 or SHA-256), checked against every case once the
            Hash Calculator plugin has run. Matches appear under Files &rarr; Hash Set Hits. Known-bad
            lists flag files (e.g. Project VIC, CAID); known-good lists such as NSRL mark files that
            are usually safe to set aside. Stored on this computer for all cases.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-2 border-b px-3 py-2 text-xs">
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              disabled={isWorking}
              onClick={() => void chooseFile()}
            >
              Choose file&hellip;
            </Button>
            <span className="min-w-0 flex-1 truncate text-muted-foreground" title={sourcePath}>
              {sourcePath || "Plain list, CSV (including NSRL), hashdeep or JSON export"}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <Input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Name, e.g. CAID 2025-09"
              className="h-7 flex-1 rounded-sm text-xs"
              aria-label="Hash set name"
            />
            <select
              value={kind}
              onChange={(event) => setKind(event.target.value as HashSetKind)}
              className="h-7 rounded-sm border bg-background px-1 text-xs"
              aria-label="Hash set kind"
            >
              <option value="known-bad">Known bad (flag)</option>
              <option value="known-good">Known good (ignorable)</option>
            </select>
            <Button
              type="button"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              disabled={isWorking || !sourcePath || !name.trim()}
              onClick={() => void runImport()}
            >
              <Upload className="size-3.5" aria-hidden="true" />
              {isWorking ? "Working…" : "Import"}
            </Button>
          </div>
        </div>

        <ul className="grid max-h-72 gap-1 overflow-auto px-3 py-2 text-xs">
          {sets.length === 0 && (
            <li className="text-muted-foreground">No hash sets imported yet.</li>
          )}
          {sets.map((set) => (
            <li key={set.id} className="flex items-center gap-2">
              {set.kind === "known-bad" ? (
                <ShieldAlert className="size-3.5 text-destructive" aria-hidden="true" />
              ) : (
                <ShieldCheck className="size-3.5 text-muted-foreground" aria-hidden="true" />
              )}
              <span className="font-medium">{set.name}</span>
              <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px] font-normal">
                {set.kind === "known-bad" ? "known bad" : "known good"}
              </Badge>
              <span className="text-muted-foreground">
                {set.entryCount.toLocaleString()} hashes &middot; imported{" "}
                {new Date(Number(set.importedAt)).toLocaleDateString()}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="xs"
                className="ml-auto h-6 rounded-sm px-1.5 text-xs"
                disabled={isWorking}
                onClick={() => void remove(set)}
                aria-label={`Delete hash set ${set.name}`}
              >
                <Trash2 className="size-3.5" aria-hidden="true" />
              </Button>
            </li>
          ))}
        </ul>
      </DialogContent>
    </Dialog>
  );
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
