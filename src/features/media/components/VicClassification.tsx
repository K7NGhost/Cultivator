import { Button } from "@/components/ui/button";
import type { ReactNode } from "react";
import { VIC_CATEGORIES, toggleVicFilter, type ClassificationDefinition, type VicCategory, type VicFilterValue } from "../model/vicClassification";
import { CustomClassificationManager, type CustomClassificationActions } from "./CustomClassificationManager";

type ControlsProps = {
  customCategories?: ClassificationDefinition[];
  category?: VicCategory | null;
  disabled?: boolean;
  onClassify: (category: VicCategory | null) => void;
};
export function VicClassificationControls({ category, disabled, onClassify, customCategories = [] }: ControlsProps) {
  return (
    <section className="grid gap-1 border-b pb-2" aria-label="Classify selected image">
      <h3 className="text-xs font-medium">Image classification</h3>
        <p className="text-[11px] text-muted-foreground">Assign a category to the selected image. Keys 0–4 also work on a focused tile.</p>
      {[...VIC_CATEGORIES, ...customCategories].map((entry) => (
        <button key={entry.value} type="button" disabled={disabled} aria-pressed={category === entry.value}
          className="flex min-h-7 items-center gap-2 rounded-sm border px-2 py-1 text-left text-[11px] hover:bg-accent disabled:opacity-50"
          style={{ borderColor: entry.color, backgroundColor: category === entry.value ? `${entry.color}22` : undefined }}
          onClick={() => onClassify(entry.value)}>
          {entry.value <= 4 ? <span className="font-mono font-semibold" style={{ color: entry.color }}>{entry.value}</span>
            : <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: entry.color }} />}
          <span className="min-w-0 break-words">{entry.label}</span>
        </button>
      ))}
      <Button size="xs" variant="ghost" className="justify-start" type="button"
        disabled={disabled || category == null} onClick={() => onClassify(null)}>Clear classification</Button>
    </section>
  );
}

export function VicFilterSidebar({ filter, counts, onChange, disabled, custom, directorySearch }: {
  filter: VicFilterValue[]; counts: number[]; onChange: (value: VicFilterValue[]) => void; disabled?: boolean;
  custom?: CustomClassificationActions;
  directorySearch?: ReactNode;
}) {
  const entries = [{ value: -1 as const, label: "Unclassified", color: "#94a3b8" }, ...VIC_CATEGORIES, ...(custom?.categories ?? [])];
  return (
    <aside className="flex min-h-0 flex-col border-r bg-muted/20" aria-label="Image classification filters">
      <div className="flex h-8 shrink-0 items-center border-b px-2 text-xs font-medium">Filters</div>
      {directorySearch}
      <div className="grid content-start gap-2 overflow-auto p-2 text-xs">
        <h3 className="font-medium">Classifications</h3>
        <button type="button" disabled={disabled} aria-pressed={!filter.length} onClick={() => onChange([])}
          className="flex items-center justify-between rounded-sm border px-2 py-1.5 text-left hover:bg-accent data-[active=true]:bg-accent"
          data-active={!filter.length}>
          <span>All images</span><span className="font-mono text-muted-foreground">{counts.reduce((sum, count) => sum + count, 0).toLocaleString()}</span>
        </button>
        {entries.map((entry) => (
          <label key={entry.value} className="flex cursor-pointer items-start gap-2 rounded-sm border-l-2 px-1.5 py-1.5 hover:bg-accent"
            style={{ borderColor: entry.color }}>
            <input type="checkbox" className="mt-0.5 shrink-0 accent-primary" checked={filter.includes(entry.value)} disabled={disabled}
              onChange={() => onChange(toggleVicFilter(filter, entry.value))} />
            <span className="min-w-0 flex-1 break-words leading-4">{entry.value >= 0 && entry.value <= 4 ? `${entry.value} · ` : ""}{entry.label}</span>
            <span className="font-mono text-[11px] text-muted-foreground">{(counts[entry.value + 1] ?? 0).toLocaleString()}</span>
          </label>
        ))}
        <p className="border-t pt-2 text-[11px] text-muted-foreground">Select categories to show. No categories selected shows all images.</p>
        {custom && <CustomClassificationManager {...custom} disabled={disabled || custom.disabled} />}
      </div>
    </aside>
  );
}
