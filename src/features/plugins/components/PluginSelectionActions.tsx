import { CheckCheck, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  areAllSelected,
  deselectPlugins,
  selectPlugins,
} from "@/features/plugins/pluginSelection";

/**
 * "Select all" / "Clear" for a plugin checklist. Both act on the plugins
 * currently shown, so a filtered list ("Android") can be ticked in one click.
 */
export function PluginSelectionActions({
  disabled = false,
  onChange,
  pluginIds,
  selectedIds,
}: {
  disabled?: boolean;
  onChange: (nextSelectedIds: string[]) => void;
  pluginIds: string[];
  selectedIds: string[];
}) {
  const allSelected = areAllSelected(selectedIds, pluginIds);
  const noneSelected = !pluginIds.some((pluginId) => selectedIds.includes(pluginId));

  return (
    <div className="flex items-center gap-1">
      <Button
        type="button"
        variant="outline"
        size="xs"
        className="h-6 rounded-sm px-2 text-[11px]"
        disabled={disabled || allSelected}
        onClick={() => onChange(selectPlugins(selectedIds, pluginIds))}
      >
        <CheckCheck className="size-3.5" aria-hidden="true" />
        Select all
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="xs"
        className="h-6 rounded-sm px-2 text-[11px]"
        disabled={disabled || noneSelected}
        onClick={() => onChange(deselectPlugins(selectedIds, pluginIds))}
      >
        <X className="size-3.5" aria-hidden="true" />
        Clear
      </Button>
    </div>
  );
}
