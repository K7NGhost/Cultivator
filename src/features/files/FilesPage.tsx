import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Tree, type NodeApi, type NodeRendererProps } from "react-arborist";
import {
  Grid,
  Willow,
  WillowDark,
  type ICellProps,
  type IColumnConfig,
} from "@svar-ui/react-grid";
import {
  AlertCircle,
  ArrowLeft,
  ChevronRight,
  CheckCircle2,
  Clock3,
  File,
  FileCode2,
  FileImage,
  FolderOpen,
  Search,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useTheme } from "@/components/theme-provider";
import { Input } from "@/components/ui/input";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import {
  type EvidenceDirectoryEntry,
  type EvidenceTreeNode,
  useEvidence,
} from "@/features/evidence/evidence-provider";
import { cn } from "@/lib/utils";

const pluginQueue = [
  { name: "Browser History Extractor", target: "History", state: "Complete" },
  { name: "Credential Store Parser", target: "Login Data", state: "Queued" },
  { name: "EXIF Metadata Reader", target: "IMG_2044.jpg", state: "Complete" },
  { name: "Keyword Scanner", target: "notes.txt", state: "Ready" },
];

function formatFileSize(size?: number) {
  if (size === undefined) {
    return "-";
  }

  if (size < 1024) {
    return `${size} B`;
  }

  const units = ["KB", "MB", "GB", "TB"];
  let value = size / 1024;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unitIndex]}`;
}

function formatModifiedTime(modifiedMs?: number) {
  if (modifiedMs === undefined) {
    return "-";
  }

  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(modifiedMs));
}

function getEntryType(entry: EvidenceDirectoryEntry) {
  if (entry.kind === "directory") {
    return "Folder";
  }

  const extension = entry.name.split(".").pop();

  return extension && extension !== entry.name ? extension.toUpperCase() : "File";
}

function getEntryPlugin(entry: EvidenceDirectoryEntry) {
  if (entry.kind === "directory") {
    return "Directory traversal";
  }

  const extension = entry.name.split(".").pop()?.toLowerCase();

  switch (extension) {
    case "jpg":
    case "jpeg":
    case "png":
      return "Image Metadata Reader";
    case "json":
      return "JSON Extractor";
    case "sqlite":
    case "db":
      return "SQLite Parser";
    case "txt":
    case "log":
      return "Keyword Scanner";
    default:
      return "File Classifier";
  }
}

function getEntryIcon(entry: EvidenceDirectoryEntry) {
  if (entry.kind === "directory") {
    return FolderOpen;
  }

  const extension = entry.name.split(".").pop()?.toLowerCase();

  if (extension === "jpg" || extension === "jpeg" || extension === "png") {
    return FileImage;
  }

  if (extension === "json" || extension === "db" || extension === "sqlite") {
    return FileCode2;
  }

  return File;
}

function getEntryIconClassName(entry: EvidenceDirectoryEntry) {
  if (entry.kind === "directory") {
    return "text-amber-600 dark:text-amber-400";
  }

  const extension = entry.name.split(".").pop()?.toLowerCase();

  if (extension === "jpg" || extension === "jpeg" || extension === "png") {
    return "text-violet-600 dark:text-violet-400";
  }

  if (extension === "json") {
    return "text-emerald-600 dark:text-emerald-400";
  }

  if (extension === "db" || extension === "sqlite") {
    return "text-blue-600 dark:text-blue-400";
  }

  if (extension === "txt" || extension === "log") {
    return "text-slate-600 dark:text-slate-300";
  }

  return "text-muted-foreground";
}

function getEntryBadgeClassName(entry: EvidenceDirectoryEntry) {
  if (entry.kind === "directory") {
    return "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300";
  }

  const extension = entry.name.split(".").pop()?.toLowerCase();

  if (extension === "jpg" || extension === "jpeg" || extension === "png") {
    return "bg-violet-500/10 text-violet-700 dark:text-violet-300";
  }

  if (extension === "json") {
    return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  }

  if (extension === "db" || extension === "sqlite") {
    return "bg-blue-500/10 text-blue-700 dark:text-blue-300";
  }

  return "";
}

type FileGridRow = {
  id: string;
  name: string;
  path: string;
  kind: EvidenceDirectoryEntry["kind"];
  type: string;
  sizeLabel: string;
  sizeValue: number;
  modifiedLabel: string;
  modifiedValue: number;
  plugin: string;
  status: string;
  extension: string;
  childCount?: number;
  openFolder?: () => void;
};

function NameCell({ row }: ICellProps) {
  const entry = row as FileGridRow;
  const Icon = getEntryIcon(entry);

  return (
    <div
      className={cn(
        "flex h-full min-w-0 items-center gap-1.5 px-1 text-xs",
        entry.kind === "directory" && "cursor-default",
      )}
      onDoubleClick={() => {
        entry.openFolder?.();
      }}
    >
      <Icon
        className={cn("size-3.5 shrink-0", getEntryIconClassName(entry))}
        aria-hidden="true"
      />
      <div className="min-w-0 truncate font-medium">{entry.name}</div>
    </div>
  );
}

function StatusCell({ row }: ICellProps) {
  const entry = row as FileGridRow;

  return (
    <Badge
      variant={entry.kind === "directory" ? "outline" : "secondary"}
      className={cn(
        "h-5 rounded-sm px-1.5 text-[11px]",
        getEntryBadgeClassName(entry),
      )}
    >
      {entry.status}
    </Badge>
  );
}

const fileGridColumns: IColumnConfig[] = [
  {
    id: "name",
    header: [
      { text: "Name" },
      { filter: { type: "text", config: { clear: true } } },
    ],
    width: 280,
    sort: true,
    resize: true,
    cell: NameCell,
    tooltip: true,
  },
  {
    id: "path",
    header: [
      { text: "Path" },
      { filter: { type: "text", config: { clear: true } } },
    ],
    width: 320,
    sort: true,
    resize: true,
    tooltip: true,
  },
  {
    id: "type",
    header: [
      { text: "Type" },
      { filter: { type: "text", config: { clear: true } } },
    ],
    width: 90,
    sort: true,
    resize: true,
  },
  {
    id: "sizeLabel",
    header: [
      { text: "Size" },
      { filter: { type: "text", config: { clear: true } } },
    ],
    width: 96,
    sort: (a, b) => {
      const left = Number(a.sizeValue ?? 0);
      const right = Number(b.sizeValue ?? 0);

      return left === right ? 0 : left > right ? 1 : -1;
    },
    resize: true,
  },
  {
    id: "modifiedLabel",
    header: [
      { text: "Modified" },
      { filter: { type: "text", config: { clear: true } } },
    ],
    width: 150,
    sort: (a, b) => {
      const left = Number(a.modifiedValue ?? 0);
      const right = Number(b.modifiedValue ?? 0);

      return left === right ? 0 : left > right ? 1 : -1;
    },
    resize: true,
  },
  {
    id: "plugin",
    header: [
      { text: "Plugin" },
      { filter: { type: "text", config: { clear: true } } },
    ],
    width: 170,
    sort: true,
    resize: true,
    tooltip: true,
  },
  {
    id: "status",
    header: [
      { text: "Status" },
      { filter: { type: "text", config: { clear: true } } },
    ],
    width: 92,
    sort: true,
    resize: true,
    cell: StatusCell,
  },
];

const fileGridMinimumWidth = fileGridColumns.reduce((total, column) => {
  return total + Number(column.width ?? 120);
}, 0);

function createFileGridColumns(width: number): IColumnConfig[] {
  const usableWidth = Math.max(width, fileGridMinimumWidth);
  const widthRatio = usableWidth / fileGridMinimumWidth;

  return fileGridColumns.map((column) => ({
    ...column,
    width: Math.max(
      Number(column.width ?? 120),
      Math.floor(Number(column.width ?? 120) * widthRatio),
    ),
  }));
}

function getViewportWidth() {
  if (typeof document === "undefined") {
    return Number.MAX_SAFE_INTEGER;
  }

  return document.documentElement.clientWidth;
}

function createFileGridRows(
  entries: EvidenceDirectoryEntry[],
  openFolder: (entry: EvidenceDirectoryEntry) => void,
): FileGridRow[] {
  return entries.map((entry) => ({
    id: entry.id,
    name: entry.name,
    path: entry.path,
    kind: entry.kind,
    type: getEntryType(entry),
    sizeLabel:
      entry.kind === "directory"
        ? `${entry.childCount ?? 0} items`
        : formatFileSize(entry.size),
    sizeValue: entry.kind === "directory" ? 0 : entry.size ?? 0,
    modifiedLabel: formatModifiedTime(entry.modifiedMs),
    modifiedValue: entry.modifiedMs ?? 0,
    plugin: getEntryPlugin(entry),
    status: entry.kind === "directory" ? "Folder" : "Indexed",
    extension: entry.name.split(".").pop()?.toLowerCase() ?? "",
    childCount: entry.childCount,
    openFolder:
      entry.kind === "directory"
        ? () => {
            openFolder(entry);
          }
        : undefined,
  }));
}

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

    return () => resizeObserver.disconnect();
  }, []);

  return { ref, size };
}

function EvidenceTreeNodeRow({
  node,
  style,
  onSelectDirectory,
}: NodeRendererProps<EvidenceTreeNode> & {
  onSelectDirectory: (node: EvidenceTreeNode) => void;
}) {
  const ancestorColumns: NodeApi<EvidenceTreeNode>[] = [];
  let parent = node.parent;

  while (parent && !parent.isRoot) {
    ancestorColumns.unshift(parent);
    parent = parent.parent;
  }

  const connectorWidth = node.level * 16;
  const currentLineX = Math.max(connectorWidth - 8, 0);

  return (
    <div style={style} className="relative px-1">
      <div
        className="pointer-events-none absolute inset-y-0 left-1"
        style={{ width: `${connectorWidth}px` }}
        aria-hidden="true"
      >
        {ancestorColumns.map((ancestorNode, index) => {
          if (!ancestorNode.nextSibling) {
            return null;
          }

          return (
            <span
              key={ancestorNode.id}
              className="absolute top-0 bottom-0 w-px bg-foreground"
              style={{ left: `${index * 16 + 8}px` }}
            />
          );
        })}
        {node.level > 0 && (
          <>
            <span
              className="absolute top-0 h-1/2 w-px bg-foreground"
              style={{ left: `${currentLineX}px` }}
            />
            {node.nextSibling && (
              <span
                className="absolute bottom-0 h-1/2 w-px bg-foreground"
                style={{ left: `${currentLineX}px` }}
              />
            )}
            <span
              className="absolute top-1/2 h-px w-2 bg-foreground"
              style={{ left: `${currentLineX}px` }}
            />
          </>
        )}
      </div>
      <div
        className={cn(
          "flex h-7 items-center rounded-sm",
          node.isSelected && "bg-accent",
        )}
        style={{ paddingLeft: `${connectorWidth + 6}px` }}
      >
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-5 shrink-0 rounded-sm"
          disabled={node.isLeaf}
          aria-label={node.isOpen ? "Collapse folder" : "Expand folder"}
          onClick={() => {
            if (node.isInternal) {
              node.toggle();
            }
          }}
        >
          <ChevronRight
            className={cn(
              "size-3 text-muted-foreground transition-transform",
              node.isOpen && "rotate-90",
              node.isLeaf && "invisible",
            )}
            aria-hidden="true"
          />
        </Button>
        <Button
          type="button"
          variant="ghost"
          className="h-7 min-w-0 flex-1 justify-start gap-1 rounded-sm px-1.5 text-xs font-normal"
          onClick={() => {
            node.select();
            onSelectDirectory(node.data);
          }}
        >
          <FolderOpen
            className="size-3.5 shrink-0 text-amber-600 dark:text-amber-400"
            aria-hidden="true"
          />
          <span className="min-w-0 flex-1 truncate text-left">
            {node.data.name}
          </span>
          <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
            {node.data.files}
          </Badge>
        </Button>
      </div>
    </div>
  );
}

export function FilesPage() {
  const { error, isLoading, listing, openDirectory } = useEvidence();
  const { theme } = useTheme();
  const [selectedDirectory, setSelectedDirectory] =
    useState<EvidenceTreeNode | null>(null);
  const [directoryHistory, setDirectoryHistory] = useState<EvidenceTreeNode[]>(
    [],
  );
  const [visibleEntries, setVisibleEntries] = useState<EvidenceDirectoryEntry[]>(
    [],
  );
  const [entriesError, setEntriesError] = useState<string | null>(null);
  const [isEntriesLoading, setIsEntriesLoading] = useState(false);
  const selectedEntry = visibleEntries[0] ?? null;
  const SelectedIcon = selectedEntry ? getEntryIcon(selectedEntry) : FolderOpen;
  const treePanel = useElementSize<HTMLDivElement>();
  const fileGridPanel = useElementSize<HTMLDivElement>();
  const fileGridAvailableWidth = Math.min(
    fileGridPanel.size.width,
    getViewportWidth(),
  );
  const fileGridWidth = Math.max(fileGridAvailableWidth, fileGridMinimumWidth);
  const stretchedFileGridColumns = useMemo(
    () => createFileGridColumns(fileGridAvailableWidth),
    [fileGridAvailableWidth],
  );
  const GridTheme = theme === "dark" ? WillowDark : Willow;

  useLayoutEffect(() => {
    setSelectedDirectory(listing?.tree ?? null);
    setVisibleEntries(listing?.entries ?? []);
    setDirectoryHistory([]);
    setEntriesError(null);
  }, [listing]);

  async function loadDirectoryEntries(
    node: EvidenceTreeNode,
    options: { pushHistory?: boolean } = {},
  ) {
    if (selectedDirectory?.path === node.path && options.pushHistory) {
      return;
    }

    if (options.pushHistory && selectedDirectory) {
      setDirectoryHistory((history) => [...history, selectedDirectory]);
    }

    setSelectedDirectory(node);
    setEntriesError(null);
    setIsEntriesLoading(true);

    try {
      const entries = await invoke<EvidenceDirectoryEntry[]>(
        "list_directory_entries",
        { path: node.path },
      );
      setVisibleEntries(entries);
    } catch (caughtError) {
      setEntriesError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
      setVisibleEntries([]);
    } finally {
      setIsEntriesLoading(false);
    }
  }

  function openFolderEntry(entry: EvidenceDirectoryEntry) {
    void loadDirectoryEntries(
      {
        id: entry.id,
        name: entry.name,
        path: entry.path,
        kind: entry.kind,
        files: entry.childCount ?? 0,
      },
      { pushHistory: true },
    );
  }

  function goBackDirectory() {
    const previousDirectory = directoryHistory[directoryHistory.length - 1];

    if (!previousDirectory) {
      return;
    }

    setDirectoryHistory((history) => history.slice(0, -1));
    void loadDirectoryEntries(previousDirectory);
  }

  const fileGridRows = createFileGridRows(visibleEntries, openFolderEntry);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <Button
          size="xs"
          className="h-7 px-2 text-xs"
          disabled={isLoading}
          onClick={() => {
            void openDirectory();
          }}
        >
          <FolderOpen className="size-3.5" aria-hidden="true" />
          {isLoading ? "Opening..." : "Open Directory"}
        </Button>
        <Separator orientation="vertical" className="h-5" />
        <div className="relative w-72">
          <Search
            className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            className="h-7 pl-7 text-xs"
            placeholder="Search logical files..."
          />
        </div>
        <div className="ml-auto flex items-center gap-1 text-[11px] text-muted-foreground">
          <span>{listing ? "1 directory mounted" : "0 directories mounted"}</span>
          <Separator orientation="vertical" className="h-4" />
          <span>{visibleEntries.length} visible entries</span>
        </div>
      </section>

      {(error || entriesError) && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{error ?? entriesError}</span>
        </section>
      )}

      <ResizablePanelGroup
        orientation="horizontal"
        className="min-h-0 flex-1"
      >
        <ResizablePanel defaultSize="18%" minSize="12%" maxSize="35%">
          <section className="h-full min-h-0" aria-label="Directory tree">
          <div className="flex h-8 items-center border-b px-2 text-xs font-medium uppercase text-muted-foreground">
            Evidence Tree
          </div>
          <div ref={treePanel.ref} className="h-[calc(100%-2rem)]">
            {treePanel.size.height > 0 && (
              <Tree
                data={listing ? [listing.tree] : []}
                width="100%"
                height={treePanel.size.height}
                rowHeight={28}
                indent={0}
                openByDefault
                disableDrag
                disableDrop
                selection={selectedDirectory?.id ?? listing?.tree.id}
                className="py-1"
                aria-label="Evidence directory tree"
              >
                {(props) => (
                  <EvidenceTreeNodeRow
                    {...props}
                    onSelectDirectory={(node) => {
                      void loadDirectoryEntries(node, { pushHistory: true });
                    }}
                  />
                )}
              </Tree>
            )}
          </div>
          </section>
        </ResizablePanel>

        <ResizableHandle withHandle />

        <ResizablePanel defaultSize="58%" minSize="35%">
          <section className="h-full min-h-0 min-w-0" aria-label="Logical file table">
          <div className="flex h-8 items-center justify-between gap-2 border-b px-2">
            <div className="flex min-w-0 items-center gap-2">
              <h1 className="text-xs font-medium uppercase text-muted-foreground">
                Logical Files
              </h1>
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="h-6 rounded-sm px-1.5 text-[11px]"
                disabled={directoryHistory.length === 0 || isEntriesLoading}
                onClick={goBackDirectory}
              >
                <ArrowLeft className="size-3" aria-hidden="true" />
                Back
              </Button>
            </div>
            <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
              {isEntriesLoading ? "Loading folder" : "Directory-only acquisition"}
            </Badge>
          </div>
          <div
            ref={fileGridPanel.ref}
            className="cultivator-grid h-[calc(100%-2rem)] min-w-0 max-w-full overflow-auto text-xs"
          >
            <GridTheme fonts={false}>
              <div
                className="h-full min-w-0 max-w-full"
                style={{
                  minWidth: `${fileGridMinimumWidth}px`,
                  width: `${fileGridWidth}px`,
                }}
              >
                <Grid
                  data={fileGridRows}
                  columns={stretchedFileGridColumns}
                  sizes={{
                    rowHeight: 32,
                    headerHeight: 28,
                    columnWidth: 120,
                  }}
                  selectedRows={fileGridRows[0] ? [fileGridRows[0].id] : []}
                  multiselect
                  select
                  reorder
                  autoRowHeight={false}
                  rowStyle={(row) =>
                    row.kind === "directory"
                      ? "bg-amber-500/5"
                      : "bg-background"
                  }
                />
              </div>
            </GridTheme>
          </div>
          </section>
        </ResizablePanel>

        <ResizableHandle withHandle />

        <ResizablePanel defaultSize="24%" minSize="16%" maxSize="40%">
          <aside className="h-full min-h-0 overflow-hidden" aria-label="Inspector">
          <ScrollArea className="h-full min-h-0">
            <div className="space-y-3 p-2">
              <section>
                <div className="flex items-center gap-2">
                  <div className="flex size-7 items-center justify-center rounded-sm border bg-muted">
                    <SelectedIcon
                      className={cn(
                        "size-3.5",
                        selectedEntry
                          ? getEntryIconClassName(selectedEntry)
                          : "text-amber-600 dark:text-amber-400",
                      )}
                      aria-hidden="true"
                    />
                  </div>
                  <div className="min-w-0">
                    <h2 className="truncate text-xs font-semibold">
                      {selectedEntry?.name ?? "No directory loaded"}
                    </h2>
                    <p className="text-[11px] text-muted-foreground">
                      {selectedEntry
                        ? `${getEntryType(selectedEntry)} - ${
                            selectedEntry.kind === "directory"
                              ? `${selectedEntry.childCount ?? 0} items`
                              : formatFileSize(selectedEntry.size)
                          }`
                        : selectedDirectory
                          ? "Folder selected"
                          : "Use Evidence > Open directory"}
                    </p>
                  </div>
                </div>
              </section>

              <Separator />

              <section>
                <div className="mb-2 text-xs font-medium uppercase text-muted-foreground">
                  Selected File
                </div>
                <dl className="grid grid-cols-[78px_1fr] gap-x-2 gap-y-1 text-[11px]">
                  <dt className="text-muted-foreground">Path</dt>
                  <dd className="break-all">
                    {selectedEntry?.path ?? selectedDirectory?.path ?? "-"}
                  </dd>
                  <dt className="text-muted-foreground">Modified</dt>
                  <dd>{formatModifiedTime(selectedEntry?.modifiedMs)}</dd>
                  <dt className="text-muted-foreground">Extractor</dt>
                  <dd>{selectedEntry ? getEntryPlugin(selectedEntry) : "-"}</dd>
                </dl>
              </section>

              <Separator />

              <section>
                <div className="mb-2 text-xs font-medium uppercase text-muted-foreground">
                  Plugin Queue
                </div>
                <div className="space-y-1">
                  {pluginQueue.map((job) => (
                    <div
                      key={job.name}
                      className="flex min-w-0 items-center gap-2 rounded-sm border px-2 py-1.5"
                    >
                      {job.state === "Complete" ? (
                        <CheckCircle2
                          className="size-3.5 shrink-0 text-emerald-600"
                          aria-hidden="true"
                        />
                      ) : (
                        <Clock3
                          className="size-3.5 shrink-0 text-amber-600"
                          aria-hidden="true"
                        />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-xs font-medium">
                          {job.name}
                        </div>
                        <div className="truncate text-[11px] text-muted-foreground">
                          {job.target}
                        </div>
                      </div>
                      <Badge
                        variant="outline"
                        className="h-4 rounded-sm px-1 text-[10px]"
                      >
                        {job.state}
                      </Badge>
                    </div>
                  ))}
                </div>
              </section>
            </div>
          </ScrollArea>
          </aside>
        </ResizablePanel>
      </ResizablePanelGroup>

      <footer className="flex h-6 shrink-0 items-center gap-3 border-t px-2 text-[11px] text-muted-foreground">
        <span>{isLoading || isEntriesLoading ? "Loading" : "Ready"}</span>
        <span>Evidence: {listing?.rootName ?? "none"}</span>
        <span>Folder: {selectedDirectory?.name ?? "none"}</span>
        <span>Files indexed: {visibleEntries.length}</span>
        <span>Plugin jobs: 0 running</span>
      </footer>
    </div>
  );
}
