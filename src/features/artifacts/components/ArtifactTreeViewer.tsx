import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { Tree, type NodeRendererProps, type TreeApi } from "react-arborist";
import { Car, Clock, Contact, ChevronRight, Database, FileText, FolderOpen, Image, MapPin, MessageSquare, Search, Shield, Smartphone, Table2, Trash2, User, Wifi, type LucideIcon } from "lucide-react";
import { AppWindow, ChevronsDownUp, ChevronsUpDown, Folder, Globe, KeyRound, ListTree, Mail, Monitor, Settings2, Usb, Users, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { ContextMenu, ContextMenuCheckboxItem, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { ArtifactTreeNode } from "../model/artifactTree";
import type { StoredArtifactRecord } from "../model/types";
import { formatEntryHitLabel } from "../utils/artifactEntryLabel";

function useElementSize<TElement extends HTMLElement>() {
  const ref = useRef<TElement | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  useLayoutEffect(() => {
    const element = ref.current;

    if (!element) {
      return;
    }

    const resizeObserver = new ResizeObserver(([entry]) => {
      setSize({
        width: entry.contentRect.width,
        height: entry.contentRect.height,
      });
    });

    resizeObserver.observe(element);
    setSize({
      width: element.clientWidth,
      height: element.clientHeight,
    });

    return () => resizeObserver.disconnect();
  }, []);

  return { ref, size };
}

function getArtifactTreeIcon(iconName: string | undefined): LucideIcon {
  switch (iconName?.toLowerCase()) {
    case "car":
      return Car;
    case "clock":
      return Clock;
    case "contact":
    case "contacts":
      return Contact;
    case "image":
      return Image;
    case "map-pin":
    case "map":
      return MapPin;
    case "message":
    case "message-square":
      return MessageSquare;
    case "search":
      return Search;
    case "shield":
      return Shield;
    case "smartphone":
    case "phone":
      return Smartphone;
    case "table":
    case "table-2":
      return Table2;
    case "user":
      return User;
    case "wifi":
    case "wireless":
      return Wifi;
    default:
      return FileText;
  }
}

const categoryAppearance: Record<string, { icon: LucideIcon; tone: string }> = {
  "Refined Results": { icon: Search, tone: "text-blue-600 dark:text-blue-400" },
  "Web Related": { icon: Globe, tone: "text-cyan-600 dark:text-cyan-400" },
  Communication: { icon: MessageSquare, tone: "text-blue-600 dark:text-blue-400" },
  "Social Networking": { icon: Users, tone: "text-violet-600 dark:text-violet-400" },
  Media: { icon: Image, tone: "text-fuchsia-600 dark:text-fuchsia-400" },
  "Email & Calendar": { icon: Mail, tone: "text-amber-600 dark:text-amber-400" },
  Documents: { icon: FileText, tone: "text-slate-600 dark:text-slate-400" },
  "Application Usage": { icon: AppWindow, tone: "text-indigo-600 dark:text-indigo-400" },
  "Operating System": { icon: Monitor, tone: "text-slate-600 dark:text-slate-400" },
  "Encryption & Credentials": { icon: KeyRound, tone: "text-amber-600 dark:text-amber-400" },
  "Connected Devices": { icon: Usb, tone: "text-teal-600 dark:text-teal-400" },
  "Location & Travel": { icon: MapPin, tone: "text-emerald-600 dark:text-emerald-400" },
  Custom: { icon: Table2, tone: "text-violet-600 dark:text-violet-400" },
};

export function ArtifactTreeViewer({
  artifacts,
  emptyText,
  entryCount,
  occurrenceCount,
  showEmptyArtifacts,
  onShowEmptyArtifactsChange,
  onSelectAllEvidence,
  onSelectArtifactNode,
  onRequestRemoveArtifact,
  onRequestRemoveCategory,
  selectedTreeNodeId,
  treeData,
  taggedOnly,
  onTaggedOnlyChange,
  tagFilterLoading,
  tagFilterError,
  onRetryTagFilter,
}: {
  artifacts: StoredArtifactRecord[];
  emptyText: string;
  entryCount: number;
  occurrenceCount: number;
  showEmptyArtifacts: boolean;
  onShowEmptyArtifactsChange: (showEmptyArtifacts: boolean) => void;
  onSelectAllEvidence: () => void;
  onSelectArtifactNode: (artifactNode: ArtifactTreeNode) => void;
  onRequestRemoveArtifact: (artifactNode: ArtifactTreeNode) => void;
  onRequestRemoveCategory: (categoryNode: ArtifactTreeNode) => void;
  selectedTreeNodeId: string | null;
  treeData: ArtifactTreeNode[];
  taggedOnly: boolean;
  onTaggedOnlyChange: (checked: boolean) => void;
  tagFilterLoading: boolean;
  tagFilterError: string;
  onRetryTagFilter: () => void;
}) {
  const treePanel = useElementSize<HTMLDivElement>();
  const treeRef = useRef<TreeApi<ArtifactTreeNode> | undefined>(undefined);
  const [search, setSearch] = useState("");
  const query = search.trim().toLocaleLowerCase();
  const noMatches = useMemo(() => {
    const matches = (nodes: ArtifactTreeNode[]): boolean => nodes.some((node) =>
      node.name.toLocaleLowerCase().includes(query) || matches(node.children ?? []));
    return !!query && !matches(treeData);
  }, [query, treeData]);

  return (
    <section
      className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-muted/15"
      aria-label="Artifact navigation"
    >
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-3">
        <ListTree className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <h2 className="min-w-0 flex-1 truncate text-xs font-semibold">Artifact explorer</h2>
        <div className="flex items-center gap-0.5">
          <Button type="button" variant="ghost" size="icon-xs" aria-label="Expand all artifact groups" title="Expand all"
            disabled={!treeData.length} onClick={() => treeRef.current?.openAll()}><ChevronsUpDown className="size-3.5" /></Button>
          <Button type="button" variant="ghost" size="icon-xs" aria-label="Collapse all artifact groups" title="Collapse all"
            disabled={!treeData.length} onClick={() => treeRef.current?.closeAll()}><ChevronsDownUp className="size-3.5" /></Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button type="button" variant="ghost" size="icon-xs" aria-label="Artifact tree options" title="Tree options"><Settings2 className="size-3.5" /></Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-44">
              <DropdownMenuCheckboxItem className="text-xs" checked={showEmptyArtifacts}
                onCheckedChange={(checked) => onShowEmptyArtifactsChange(checked === true)}>Show empty artifacts</DropdownMenuCheckboxItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <div className="shrink-0 px-2 pb-1.5 pt-2">
        <div className="relative">
          <Search className="pointer-events-none absolute left-2 top-2 size-3.5 text-muted-foreground" aria-hidden="true" />
          <Input className="h-7 rounded-sm bg-background pl-7 pr-7 text-xs shadow-none md:text-xs" aria-label="Filter artifact tree" placeholder="Filter artifacts…"
            value={search} onChange={(event) => setSearch(event.target.value)} onKeyDown={(event) => { if (event.key === "Escape") setSearch(""); }} />
          {search && <Button className="absolute right-0.5 top-0.5 size-6" type="button" variant="ghost" size="icon-xs"
            aria-label="Clear artifact filter" onClick={() => setSearch("")}><X className="size-3" /></Button>}
        </div>
      </div>
      <div className="shrink-0 px-2 pb-2">
      <label className="mb-1.5 flex cursor-pointer items-center gap-2 px-1 text-xs">
        <Checkbox aria-label="Show tagged artifacts only" checked={taggedOnly} onCheckedChange={(checked) => onTaggedOnlyChange(checked === true)} />
        <span>Tagged only</span>
        {tagFilterLoading && <span role="status" className="ml-auto text-[10px] text-muted-foreground">Updating tags…</span>}
      </label>
      {tagFilterError && <div role="alert" className="mb-2 text-[11px] text-destructive">{tagFilterError}<Button type="button" size="xs" variant="outline" className="ml-1 h-6 text-[11px]" onClick={onRetryTagFilter}>Retry tags</Button></div>}
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            className={cn(
              "h-9 w-full justify-start gap-2 rounded-sm border border-transparent px-2 text-xs font-medium",
              selectedTreeNodeId === null &&
                "border-primary/15 bg-primary/10 text-foreground hover:bg-primary/15",
            )}
            aria-pressed={selectedTreeNodeId === null}
            title={formatEntryHitLabel(
              entryCount,
              occurrenceCount,
              artifacts.length,
            )}
            onClick={onSelectAllEvidence}
          >
            <Database className="size-3.5 shrink-0 text-primary" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate text-left">{taggedOnly ? "Tagged evidence" : "All evidence"}</span>
            <Badge variant="secondary" className="h-5 min-w-6 justify-center rounded-sm px-1.5 text-[10px] font-medium tabular-nums">
              {entryCount.toLocaleString()}
            </Badge>
          </Button>
        </ContextMenuTrigger>
        <ContextMenuContent className="min-w-48">
          <ContextMenuCheckboxItem
            checked={showEmptyArtifacts}
            className="text-xs"
            onCheckedChange={(checked) => {
              onShowEmptyArtifactsChange(checked === true);
            }}
          >
            Show empty artifacts
          </ContextMenuCheckboxItem>
        </ContextMenuContent>
      </ContextMenu>
      </div>
      <div className="flex h-6 shrink-0 items-center justify-between border-y border-border/60 bg-muted/25 px-3 text-[10px] font-medium text-muted-foreground">
        <span>Categories</span><span>Entries</span>
      </div>
      <div ref={treePanel.ref} className="relative min-h-0 flex-1 px-1 py-1">
        {noMatches && treeData.length > 0 && <div role="status" className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 px-4 text-center text-xs text-muted-foreground">
          <Search className="size-5 opacity-50" aria-hidden="true" />
          <p>No matching artifacts.</p>
          <Button type="button" variant="outline" size="xs" onClick={() => setSearch("")}>Clear filter</Button>
        </div>}
        {treeData.length === 0 ? (
          <div className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
            {emptyText}
          </div>
        ) : (
          treePanel.size.height > 0 && (
            <Tree
              ref={treeRef}
              data={treeData}
              width="100%"
              height={Math.max(1, treePanel.size.height - 8)}
              rowHeight={32}
              indent={0}
              openByDefault={false}
              disableDrag
              disableDrop
              disableEdit
              disableMultiSelection
              searchTerm={search.trim()}
              searchMatch={(node, term) => {
                const normalized = term.trim().toLocaleLowerCase();
                for (let current: typeof node | null = node; current && !current.isRoot; current = current.parent) {
                  if (current.data.name.toLocaleLowerCase().includes(normalized)) return true;
                }
                return false;
              }}
              onActivate={(node) => { if (node.data.kind === "artifact") onSelectArtifactNode(node.data); }}
              selection={selectedTreeNodeId ?? undefined}
              aria-label="Artifacts grouped by category"
            >
              {(props) => (
                <ArtifactTreeRow
                  {...props}
                  onSelectArtifactNode={onSelectArtifactNode}
                  onRequestRemoveArtifact={onRequestRemoveArtifact}
                  onRequestRemoveCategory={onRequestRemoveCategory}
                />
              )}
            </Tree>
          )
        )}
      </div>
      <div className="flex h-7 shrink-0 items-center border-t px-3 text-[10px] text-muted-foreground">
        {search.trim() ? `Filtered by name${taggedOnly ? " and tags" : ""} · Esc to clear name` : `${treeData.length.toLocaleString()} categories · ${artifacts.length.toLocaleString()} ${taggedOnly ? "tagged " : ""}artifacts`}
      </div>
    </section>
  );
}

function ArtifactTreeRow({
  node,
  onSelectArtifactNode,
  onRequestRemoveArtifact,
  onRequestRemoveCategory,
  style,
}: NodeRendererProps<ArtifactTreeNode> & {
  onSelectArtifactNode: (artifactNode: ArtifactTreeNode) => void;
  onRequestRemoveArtifact: (artifactNode: ArtifactTreeNode) => void;
  onRequestRemoveCategory: (categoryNode: ArtifactTreeNode) => void;
}) {
  const connectorWidth = node.level * 16;
  const isCategory = node.data.kind === "category";
  const displayName = isCategory
    ? node.data.name.toLowerCase() === "other"
      ? "Custom"
      : node.data.name.replace(/[_-]+/g, " ")
    : node.data.name;
  const Icon =
    node.data.kind === "artifact"
      ? getArtifactTreeIcon(node.data.icon)
      : node.data.kind === "group"
        ? node.isOpen ? FolderOpen : Folder
        : (categoryAppearance[displayName]?.icon ?? FileText);
  const iconClassName =
    node.data.kind === "artifact"
      ? "text-muted-foreground"
      : node.data.kind === "group"
        ? "text-amber-600 dark:text-amber-400"
        : (categoryAppearance[displayName]?.tone ?? "text-muted-foreground");
  const countTitle = formatEntryHitLabel(
    node.data.entryCount,
    node.data.occurrenceCount,
    node.data.count,
  );

  const row = (
    <div style={style} className="relative h-full min-w-0 py-0.5" title={`${displayName} · ${countTitle}`}>
      {Array.from({ length: node.level }, (_, depth) => <span key={depth} aria-hidden="true"
        className="pointer-events-none absolute inset-y-0 border-l border-border/60" style={{ left: 13 + depth * 16 }} />)}
      <div
        className={cn(
          "relative flex h-full items-center rounded-sm transition-colors hover:bg-accent/70",
          node.isSelected && !isCategory && "bg-primary/10 shadow-[inset_2px_0_0_var(--primary)] hover:bg-primary/15",
          node.isFocused && "ring-1 ring-inset ring-ring/50",
          node.data.entryCount === 0 && "text-muted-foreground",
        )}
        style={{ paddingLeft: `${isCategory ? 2 : connectorWidth + 2}px` }}
      >
        {node.isLeaf ? (
          <span className="size-5 shrink-0" aria-hidden="true" />
        ) : (
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="size-5 shrink-0 rounded-sm hover:bg-transparent"
            tabIndex={-1}
            aria-expanded={node.isOpen}
            aria-label={
              `${node.isOpen ? "Collapse" : "Expand"} ${displayName}`
            }
            onClick={(event) => { event.stopPropagation(); node.toggle(); }}
          >
            <ChevronRight
              className={cn(
                "size-3.5 text-muted-foreground transition-transform",
                node.isOpen && "rotate-90",
              )}
              aria-hidden="true"
            />
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
          className={cn(
            "h-full min-w-0 flex-1 justify-start gap-2 rounded-sm px-1.5 hover:bg-transparent",
            isCategory
              ? "text-xs font-semibold"
              : "text-xs font-medium",
          )}
          tabIndex={-1}
          onClick={() => {
            if (node.data.kind === "artifact") {
              node.select();
              onSelectArtifactNode(node.data);
            } else if (node.isInternal) {
              node.toggle();
            }
          }}
        >
          <span className={cn("flex size-5 shrink-0 items-center justify-center rounded-sm", isCategory && "bg-muted/70")}>
            <Icon
              className={cn("size-3.5 shrink-0", iconClassName, node.isSelected && !isCategory && "text-primary")}
              aria-hidden="true"
            />
          </span>
          <span className="min-w-0 flex-1 truncate text-left">
            {displayName}
          </span>
          <span
            className={cn(
              "min-w-6 shrink-0 rounded-sm bg-muted/70 px-1.5 py-0.5 text-center text-[10px] font-medium tabular-nums text-muted-foreground",
              node.isSelected && !isCategory && "bg-primary/10 text-primary",
            )}
            title={countTitle}
          >
            {node.data.entryCount.toLocaleString()}
          </span>
        </Button>
      </div>
    </div>
  );

  const artifact = node.data.artifact;

  if (node.data.kind === "category") {
    return (
      <ContextMenu>
        <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
        <ContextMenuContent className="min-w-48">
          <ContextMenuItem
            variant="destructive"
            disabled={node.data.count === 0}
            onSelect={() => {
              node.select();
              onRequestRemoveCategory(node.data);
            }}
          >
            <Trash2 className="size-3.5" aria-hidden="true" />
            Remove category artifacts
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    );
  }

  if (!artifact) {
    return row;
  }

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
      <ContextMenuContent className="min-w-44">
        <ContextMenuItem
          variant="destructive"
          onSelect={() => {
            node.select();
            onSelectArtifactNode(node.data);
            onRequestRemoveArtifact(node.data);
          }}
        >
          <Trash2 className="size-3.5" aria-hidden="true" />
          Remove artifact
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

