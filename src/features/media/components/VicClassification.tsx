import { Button } from "@/components/ui/button";
import type { ReactNode } from "react";
import { NSFW_FILTERS, VIC_CATEGORIES, toggleVicFilter, type ClassificationDefinition, type VicCategory, type VicFilterValue } from "../model/vicClassification";
import { CustomClassificationManager, type CustomClassificationActions } from "./CustomClassificationManager";
import clipCategories from "../model/clipClassifications.json";
import { ChevronDown } from "lucide-react";

function FilterSection({ title, label, selected, children }: {
  title: string; label: string; selected: number; children: ReactNode;
}) {
  return (
    <details className="group border-t pt-1" aria-label={label}>
      <summary className="flex cursor-pointer list-none items-center gap-2 rounded-sm px-1 py-2 font-medium hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
        <span className="min-w-0 flex-1">{title}</span>
        {selected > 0 && <span className="text-[10px] font-normal text-muted-foreground">{selected} selected</span>}
        <ChevronDown className="size-3.5 shrink-0 transition-transform group-open:rotate-180" aria-hidden="true" />
      </summary>
      <div className="grid gap-1 pb-1">{children}</div>
    </details>
  );
}

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

export type VisualGalleryControls = {
  skinMin: number | null;
  collapseSimilar: boolean;
  stackId: string | null;
  onSkinMinChange: (value: number | null) => void;
  onCollapseSimilarChange: (value: boolean) => void;
  onOpenStack: (id: string) => void;
  onClearStack: () => void;
};

export function VicFilterSidebar({ filter, counts, onChange, disabled, custom, directorySearch, visual, showFaceBoxes = true, onShowFaceBoxesChange }: {
  filter: VicFilterValue[]; counts: number[]; onChange: (value: VicFilterValue[]) => void; disabled?: boolean;
  custom?: CustomClassificationActions;
  directorySearch?: ReactNode;
  visual?: VisualGalleryControls;
  showFaceBoxes?: boolean;
  onShowFaceBoxesChange?: (value: boolean) => void;
}) {
  const entries = [{ value: -1 as const, label: "Unclassified", color: "#94a3b8" }, ...VIC_CATEGORIES, ...(custom?.categories ?? [])];
  const clearGroup = (belongs: (value: number) => boolean) => onChange(filter.filter(value => !belongs(value)));
  return (
    <aside className="flex min-h-0 flex-col border-r bg-muted/20" aria-label="Media filters">
      <div className="flex h-8 shrink-0 items-center border-b px-2 text-xs font-medium">Filters</div>
      {directorySearch}
      <div className="grid content-start gap-2 overflow-auto p-2 text-xs">
        <button type="button" disabled={disabled} aria-pressed={!filter.length && visual?.skinMin == null && !visual?.stackId} onClick={() => { onChange([]); visual?.onSkinMinChange(null); visual?.onClearStack(); }}
          className="flex items-center justify-between rounded-sm border px-2 py-1.5 text-left hover:bg-accent data-[active=true]:bg-accent"
          data-active={!filter.length && visual?.skinMin == null && !visual?.stackId}>
          <span>All images</span><span className="font-mono text-muted-foreground">{counts.slice(0, 257).reduce((sum, count) => sum + count, 0).toLocaleString()}</span>
        </button>
        <p className="text-[11px] text-muted-foreground">Show images matching any selected filter. All images clears every filter.</p>
        <FilterSection title="Classifications" label="Manual classification filters" selected={filter.filter(value => value < 256).length}>
          <div className="flex justify-end">
            <Button size="xs" variant="ghost" type="button" aria-label="Clear classification filters"
              disabled={disabled || !filter.some(value => value < 256)} onClick={() => clearGroup(value => value < 256)}>Clear</Button>
          </div>
          {entries.map((entry) => (
          <label key={entry.value} className="flex cursor-pointer items-start gap-2 rounded-sm border-l-2 px-1.5 py-1.5 hover:bg-accent"
            style={{ borderColor: entry.color }}>
            <input type="checkbox" className="mt-0.5 shrink-0 accent-primary" checked={filter.includes(entry.value)} disabled={disabled}
              onChange={() => onChange(toggleVicFilter(filter, entry.value))} />
            <span className="min-w-0 flex-1 break-words leading-4">{entry.value >= 0 && entry.value <= 4 ? `${entry.value} · ` : ""}{entry.label}</span>
            <span className="font-mono text-[11px] text-muted-foreground">{(counts[entry.value + 1] ?? 0).toLocaleString()}</span>
          </label>
          ))}
          {custom && <CustomClassificationManager {...custom} disabled={disabled || custom.disabled} />}
        </FilterSection>
        {counts.length > 257 && <FilterSection title="YuNet detection" label="YuNet detection filters" selected={filter.includes(256) ? 1 : 0}>
          <div className="flex justify-end">
            <Button size="xs" variant="ghost" type="button" aria-label="Clear YuNet filters"
              disabled={disabled || !filter.includes(256)} onClick={() => clearGroup(value => value === 256)}>Clear</Button>
          </div>
          <label className="flex cursor-pointer items-start gap-2 rounded-sm px-1 py-1.5 hover:bg-accent">
            <input type="checkbox" className="mt-0.5 shrink-0 accent-primary" checked={filter.includes(256)} disabled={disabled}
              onChange={() => onChange(toggleVicFilter(filter, 256))} />
            <span className="flex-1 leading-4">Faces detected</span>
            <span className="font-mono text-[11px] text-muted-foreground">{(counts[257] ?? 0).toLocaleString()}</span>
          </label>
          {onShowFaceBoxesChange && <label className="flex cursor-pointer items-start gap-2 rounded-sm px-1 py-1.5 hover:bg-accent">
            <input type="checkbox" className="mt-0.5 shrink-0 accent-primary" checked={showFaceBoxes}
              onChange={event => onShowFaceBoxesChange(event.target.checked)} />
            <span className="flex-1 leading-4">Show face boxes</span>
          </label>}
        </FilterSection>}
        {counts.length > 258 && <FilterSection title="CLIP categories" label="CLIP category filters" selected={filter.filter(value => value > 256 && value < 272).length}>
          <div className="flex justify-end">
            <Button size="xs" variant="ghost" type="button" aria-label="Clear CLIP filters"
              disabled={disabled || !filter.some(value => value > 256 && value < 272)} onClick={() => clearGroup(value => value > 256 && value < 272)}>Clear</Button>
          </div>
          <p className="mb-1 text-[11px] text-muted-foreground">Saved suggestions from OpenCLIP.</p>
          {clipCategories.map(category => (
            <label key={category.id} className="flex cursor-pointer items-start gap-2 rounded-sm px-1 py-1.5 hover:bg-accent"
              title={category.examples.join(", ")}>
              <input type="checkbox" className="mt-0.5 shrink-0 accent-primary" aria-label={`CLIP: ${category.label}`}
                checked={filter.includes(256 + category.id)} disabled={disabled}
                onChange={() => onChange(toggleVicFilter(filter, 256 + category.id))} />
              <span className="min-w-0 flex-1 break-words leading-4">{category.label}</span>
              <span className="font-mono text-[11px] text-muted-foreground">{(counts[257 + category.id] ?? 0).toLocaleString()}</span>
            </label>
          ))}
        </FilterSection>}
        {counts.length > 273 && <FilterSection title="NSFW" label="NSFW classifier filters" selected={filter.filter(value => value >= 272 && value <= 274).length}>
          <div className="flex justify-end">
            <Button size="xs" variant="ghost" type="button" aria-label="Clear NSFW filters"
              disabled={disabled || !filter.some(value => value >= 272 && value <= 274)} onClick={() => clearGroup(value => value >= 272 && value <= 274)}>Clear</Button>
          </div>
          <p className="mb-1 text-[11px] text-muted-foreground">Images flagged by the dedicated NSFW classifier (score ? 0.5).</p>
          {NSFW_FILTERS.map(entry => (
            <label key={entry.value} className="flex cursor-pointer items-start gap-2 rounded-sm px-1 py-1.5 hover:bg-accent">
              <input type="checkbox" className="mt-0.5 shrink-0 accent-primary" aria-label={`NSFW: ${entry.label}`}
                checked={filter.includes(entry.value)} disabled={disabled}
                onChange={() => onChange(toggleVicFilter(filter, entry.value))} />
              <span className="min-w-0 flex-1 leading-4">{entry.label}</span>
              <span className="font-mono text-[11px] text-muted-foreground">{(counts[entry.value + 1] ?? 0).toLocaleString()}</span>
            </label>
          ))}
        </FilterSection>}
        {visual && <>
          <FilterSection title="Skin-tone ratio" label="Skin-tone filters" selected={visual.skinMin === null ? 0 : 1}>
            <select aria-label="Minimum skin-tone ratio" className="h-8 rounded-sm border bg-background px-2" disabled={disabled}
              value={visual.skinMin ?? 0} onChange={event => visual.onSkinMinChange(Number(event.target.value) || null)}>
              <option value={0}>Any</option><option value={0.1}>10% or more</option>
              <option value={0.3}>30% or more</option><option value={0.5}>50% or more</option>
            </select>
            <p className="text-[11px] text-muted-foreground">Broad color screening, not a content classification. Run skin-tone screening in Image Metadata first.</p>
          </FilterSection>
          <FilterSection title="Similar images" label="Similar image stacking" selected={visual.stackId ? 1 : 0}>
            <label className="flex cursor-pointer items-start gap-2 px-1 py-1.5">
              <input type="checkbox" className="mt-0.5 accent-primary" disabled={disabled} checked={visual.collapseSimilar}
                onChange={event => visual.onCollapseSimilarChange(event.target.checked)} />
              <span>Collapse similar images</span>
            </label>
            {visual.stackId && <Button size="xs" variant="outline" disabled={disabled} onClick={visual.onClearStack}>Leave stack</Button>}
            <p className="text-[11px] text-muted-foreground">Run visual stacking in Image Metadata first. Stacks keep every original and its classification. Filter counts show individual images.</p>
          </FilterSection>
        </>}
      </div>
    </aside>
  );
}
