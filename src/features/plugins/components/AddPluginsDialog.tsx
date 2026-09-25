import { useState } from "react";
import { Play, Plus } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { DataSourceRecord } from "@/features/datasources/types";
import type { PythonPlugin } from "@/features/plugins/types";

/**
 * Lets the examiner add plugins to a datasource after it was created, and
 * optionally run just the new ones right away.
 */
export function AddPluginsDialog({
  canRun,
  datasource,
  isAdding,
  onAdd,
  onClose,
  plugins,
}: {
  canRun: boolean;
  datasource: DataSourceRecord | null;
  isAdding: boolean;
  onAdd: (pluginIds: string[], runAfterAdding: boolean) => void;
  onClose: () => void;
  plugins: PythonPlugin[];
}) {
  const [chosenIds, setChosenIds] = useState<string[]>([]);
  const alreadySelected = new Set(datasource?.pluginIds ?? []);
  const available = plugins.filter((plugin) => !alreadySelected.has(plugin.id));
  const alreadyAdded = plugins.filter((plugin) => alreadySelected.has(plugin.id));

  return (
    <Dialog
      open={Boolean(datasource)}
      onOpenChange={(isOpen) => {
        if (!isOpen && !isAdding) {
          onClose();
        }
      }}
    >
      <DialogContent className="max-w-lg rounded-sm p-0">
        <DialogHeader className="border-b px-3 py-2">
          <DialogTitle className="text-sm">Add Plugins</DialogTitle>
          <DialogDescription className="text-xs">
            Choose plugins to add to {datasource?.name ?? "this datasource"}.
            They are saved on the datasource, so later runs include them too.
          </DialogDescription>
        </DialogHeader>
        <div className="max-h-[min(28rem,calc(100vh-14rem))] overflow-auto px-3 py-2 text-xs">
          {available.length > 0 ? (
            <div className="divide-y rounded-sm border">
              {available.map((plugin) => (
                <label
                  key={plugin.id}
                  className="grid cursor-pointer grid-cols-[1rem_minmax(0,1fr)] items-start gap-2 px-2 py-2 hover:bg-accent"
                >
                  <Checkbox
                    checked={chosenIds.includes(plugin.id)}
                    onCheckedChange={(checked) => {
                      setChosenIds((current) =>
                        checked === true
                          ? [...current, plugin.id]
                          : current.filter((pluginId) => pluginId !== plugin.id),
                      );
                    }}
                  />
                  <span className="min-w-0 space-y-1">
                    <span className="block truncate font-medium leading-none">
                      {plugin.name}
                    </span>
                    <span className="block truncate text-[11px] text-muted-foreground">
                      {plugin.description || plugin.id}
                    </span>
                  </span>
                </label>
              ))}
            </div>
          ) : (
            <div className="rounded-sm border border-dashed px-2 py-4 text-center text-muted-foreground">
              Every installed plugin is already on this datasource.
            </div>
          )}
          {alreadyAdded.length > 0 && (
            <div className="mt-2 text-[11px] text-muted-foreground">
              Already added: {alreadyAdded.map((plugin) => plugin.name).join(", ")}
            </div>
          )}
        </div>
        <DialogFooter className="border-t p-2">
          <Button
            type="button"
            variant="outline"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={isAdding}
            onClick={onClose}
          >
            Cancel
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={isAdding || chosenIds.length === 0}
            onClick={() => onAdd(chosenIds, false)}
          >
            <Plus className="size-3.5" aria-hidden="true" />
            Add only
          </Button>
          <Button
            type="button"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={isAdding || chosenIds.length === 0 || !canRun}
            onClick={() => onAdd(chosenIds, true)}
          >
            <Play className="size-3.5" aria-hidden="true" />
            Add and run{chosenIds.length > 0 ? ` (${chosenIds.length})` : ""}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
