import type { ReactNode } from "react";
import { Tag } from "lucide-react";
import { ContextMenu, ContextMenuCheckboxItem, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
import type { ArtifactRowTag } from "../services/artifactTagRepository";

type Props = {
  tags: ArtifactRowTag[]; availableTags: string[]; occurrences: number; disabled: boolean;
  onToggle: (name: string, assigned: boolean) => void; onCustom: () => void; onSelect: () => void;
};

function Items({ context = false, tags, availableTags, occurrences, disabled, onToggle, onCustom }: Props & { context?: boolean }) {
  const Item = context ? ContextMenuItem : DropdownMenuItem;
  const CheckboxItem = context ? ContextMenuCheckboxItem : DropdownMenuCheckboxItem;
  const Separator = context ? ContextMenuSeparator : DropdownMenuSeparator;
  const names = new Map<string, string>();
  for (const name of ["Bookmark", "Follow-up", "Notable", ...tags.map((tag) => tag.name), ...availableTags]) names.set(name.toLocaleLowerCase(), name);
  return <>
    <div className="px-2 py-1 text-[11px] text-muted-foreground">Row tags{occurrences > 1 ? ` · ${occurrences} occurrences` : ""}</div>
    <Item className="text-xs" disabled={disabled} onSelect={onCustom}><Tag className="size-3.5" />Add custom tag…</Item>
    <Separator />
    <div className="max-h-60 overflow-y-auto">{[...names.values()].map((name) => {
      const tag = tags.find((tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase());
      const checked = tag ? tag.occurrences >= occurrences ? true : "indeterminate" : false;
      return <CheckboxItem key={name} className="text-xs" checked={checked} disabled={disabled}
        onCheckedChange={() => onToggle(name, checked !== true)}>{name}{tag && checked === "indeterminate" && <span className="ml-auto text-[10px] text-muted-foreground">{tag.occurrences}/{occurrences}</span>}</CheckboxItem>;
    })}</div>
  </>;
}

export function ArtifactRowContextMenu({ children, ...props }: Props & { children: ReactNode }) {
  return <ContextMenu><ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
    <ContextMenuContent className="min-w-48"><Items {...props} context /></ContextMenuContent>
  </ContextMenu>;
}

export function ArtifactRowTagButton(props: Props) {
  return <DropdownMenu onOpenChange={(open) => { if (open) props.onSelect(); }}>
    <DropdownMenuTrigger asChild><Button type="button" size="icon" variant="ghost" className="size-6 shrink-0 text-muted-foreground hover:text-foreground" disabled={props.disabled} aria-label="Tag artifact row"><Tag className="size-3.5" /></Button></DropdownMenuTrigger>
    <DropdownMenuContent align="end" className="min-w-48"><Items {...props} /></DropdownMenuContent>
  </DropdownMenu>;
}
