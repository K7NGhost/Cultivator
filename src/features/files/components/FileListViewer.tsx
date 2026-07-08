import {
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  AutoSizer,
  List,
  type ListRowProps,
} from "react-virtualized";
import "react-virtualized/styles.css";
import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  ArrowUpDown,
  Bookmark,
  ChevronLeft,
  ChevronRight,
  File,
  FileCode2,
  FileImage,
  Flag,
  FolderOpen,
  MessageSquarePlus,
  Plus,
  Tag,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";
import type {
  FileTagGroup,
  FileTagRecord,
} from "@/features/files/fileTagRepository";
import { cn } from "@/lib/utils";

type FileListViewerProps = {
  entries: EvidenceDirectoryEntry[];
  isLoading: boolean;
  selectedEntry: EvidenceDirectoryEntry | null;
  title?: string;
  statusLabel?: string;
  emptyLabel?: string;
  canGoBack: boolean;
  pageInfo?: FileListPageInfo | null;
  isPageLoading?: boolean;
  onGoBack: () => void;
  onNextPage?: () => void;
  onOpenFolder: (entry: EvidenceDirectoryEntry) => void;
  onPreviousPage?: () => void;
  onRemoveTag?: (entry: EvidenceDirectoryEntry, tagName: string) => void;
  onSelectEntry: (entry: EvidenceDirectoryEntry) => void;
  onSetTag?: (input: {
    entry: EvidenceDirectoryEntry;
    tagName: string;
    tagGroup: FileTagGroup;
    comment?: string;
  }) => void;
  tagsByPath?: Record<string, FileTagRecord[]>;
};

type FileListPageInfo = {
  offset: number;
  limit: number;
  totalCount: number;
  hasNextPage: boolean;
};

type FileRow = {
  entry: EvidenceDirectoryEntry;
  id: string;
  name: string;
  path: string;
  type: string;
  sizeValue: number;
  modifiedValue: number;
  plugin: string;
  status: string;
};

type SortKey = "name" | "path" | "type" | "size" | "modified" | "plugin" | "status";
type SortDirection = "asc" | "desc";

type SortState = {
  key: SortKey;
  direction: SortDirection;
};

const columns: Array<{
  key: SortKey;
  label: string;
  className: string;
}> = [
  { key: "name", label: "Name", className: "w-[280px] min-w-[280px]" },
  { key: "path", label: "Path", className: "w-[320px] min-w-[320px]" },
  { key: "type", label: "Type", className: "w-[90px] min-w-[90px]" },
  { key: "size", label: "Size", className: "w-[96px] min-w-[96px]" },
  {
    key: "modified",
    label: "Modified",
    className: "w-[150px] min-w-[150px]",
  },
  { key: "plugin", label: "Plugin", className: "w-[170px] min-w-[170px]" },
  { key: "status", label: "Status", className: "w-[92px] min-w-[92px]" },
];
const fileListGridTemplateColumns = "280px 320px 90px 96px 150px 170px 92px";

function formatCount(value: number) {
  return new Intl.NumberFormat().format(value);
}

function normalizePath(path: string) {
  return path.replace(/\\/g, "/");
}

function getTagsForEntry(
  tagsByPath: Record<string, FileTagRecord[]>,
  entry: EvidenceDirectoryEntry,
) {
  return tagsByPath[normalizePath(entry.path)] ?? [];
}

function hasTag(tags: FileTagRecord[], tagName: string) {
  return tags.some((tag) => tag.tagName === tagName);
}

function formatPageRange(pageInfo: FileListPageInfo, visibleCount: number) {
  if (pageInfo.totalCount === 0) {
    return "0 of 0";
  }

  const start = pageInfo.offset + 1;
  const end = Math.min(pageInfo.offset + visibleCount, pageInfo.totalCount);

  return `${formatCount(start)}-${formatCount(end)} of ${formatCount(pageInfo.totalCount)}`;
}

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
  if (entry.id.startsWith("file-view:")) {
    return "File View";
  }

  if (entry.kind === "directory") {
    return "Folder";
  }

  const extension = entry.name.split(".").pop();

  return extension && extension !== entry.name
    ? extension.toUpperCase()
    : "File";
}

function getEntryPlugin(entry: EvidenceDirectoryEntry) {
  if (entry.kind === "directory" || entry.id.startsWith("file-view:")) {
    return "-";
  }

  const extension = entry.name.split(".").pop()?.toLowerCase();

  switch (extension) {
    case "jpg":
    case "jpeg":
    case "png":
      return "Image Metadata Reader";
    default:
      return "-";
  }
}

function getEntrySizeLabel(entry: EvidenceDirectoryEntry) {
  if (entry.id.startsWith("file-view:")) {
    return `${formatCount(entry.childCount ?? 0)} files`;
  }

  if (entry.kind === "directory") {
    return `${formatCount(entry.childCount ?? 0)} items`;
  }

  return formatFileSize(entry.size);
}

function getEntryStatus(entry: EvidenceDirectoryEntry) {
  if (entry.id.startsWith("file-view:")) {
    return "View";
  }

  return entry.kind === "directory" ? "Folder" : "Indexed";
}

function getEntryIcon(entry: Pick<EvidenceDirectoryEntry, "kind" | "name">) {
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

function getEntryIconClassName(
  entry: Pick<EvidenceDirectoryEntry, "kind" | "name">,
) {
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

function createFileRows(entries: EvidenceDirectoryEntry[]): FileRow[] {
  return entries.map((entry) => ({
    entry,
    id: entry.id,
    name: entry.name,
    path: entry.path,
    type: getEntryType(entry),
    sizeValue: entry.kind === "directory" ? 0 : (entry.size ?? 0),
    modifiedValue: entry.modifiedMs ?? 0,
    plugin: getEntryPlugin(entry),
    status: getEntryStatus(entry),
  }));
}

function compareStrings(left: string, right: string) {
  return left.localeCompare(right, undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

function getSortValue(row: FileRow, key: SortKey) {
  switch (key) {
    case "size":
      return row.sizeValue;
    case "modified":
      return row.modifiedValue;
    case "name":
      return row.name;
    case "path":
      return row.path;
    case "type":
      return row.type;
    case "plugin":
      return row.plugin;
    case "status":
      return row.status;
  }
}

function sortFileRows(rows: FileRow[], sort: SortState) {
  if (sort.key === "name" && sort.direction === "asc") {
    return rows;
  }

  return [...rows].sort((left, right) => {
    const leftValue = getSortValue(left, sort.key);
    const rightValue = getSortValue(right, sort.key);
    const comparison =
      typeof leftValue === "number" && typeof rightValue === "number"
        ? leftValue - rightValue
        : compareStrings(String(leftValue), String(rightValue));

    return sort.direction === "asc" ? comparison : -comparison;
  });
}

function isInteractiveEventTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  return Boolean(
    target.closest(
      "button,a,input,select,textarea,[role='button'],[role='menuitem']",
    ),
  );
}

function getNextSort(current: SortState, key: SortKey): SortState {
  if (current.key !== key) {
    return { key, direction: "asc" };
  }

  return {
    key,
    direction: current.direction === "asc" ? "desc" : "asc",
  };
}

function SortIcon({
  columnKey,
  sort,
}: {
  columnKey: SortKey;
  sort: SortState;
}) {
  if (sort.key !== columnKey) {
    return <ArrowUpDown className="size-3 text-muted-foreground" aria-hidden="true" />;
  }

  return sort.direction === "asc" ? (
    <ArrowUp className="size-3 text-foreground" aria-hidden="true" />
  ) : (
    <ArrowDown className="size-3 text-foreground" aria-hidden="true" />
  );
}

function FileNameCell({ row, tags }: { row: FileRow; tags: FileTagRecord[] }) {
  const Icon = getEntryIcon(row.entry);
  const visibleTags = tags.slice(0, 2);
  const hiddenTagCount = Math.max(0, tags.length - visibleTags.length);

  return (
    <div className="flex min-w-0 items-center gap-1.5">
      <Icon
        className={cn("size-3.5 shrink-0", getEntryIconClassName(row.entry))}
        aria-hidden="true"
      />
      <span className="min-w-0 truncate font-medium">{row.name}</span>
      {visibleTags.map((tag) => (
        <Badge
          key={tag.tagName}
          variant={tag.tagGroup === "bookmark" ? "default" : "secondary"}
          className="h-4 shrink-0 rounded-sm px-1 text-[10px]"
          title={tag.comment || tag.tagName}
        >
          {tag.tagName}
        </Badge>
      ))}
      {hiddenTagCount > 0 && (
        <Badge
          variant="outline"
          className="h-4 shrink-0 rounded-sm px-1 text-[10px]"
        >
          +{hiddenTagCount}
        </Badge>
      )}
    </div>
  );
}

function StatusBadge({ row }: { row: FileRow }) {
  return (
    <Badge
      variant={row.entry.kind === "directory" ? "outline" : "secondary"}
      className={cn(
        "h-5 rounded-sm px-1.5 text-[11px]",
        getEntryBadgeClassName(row.entry),
      )}
    >
      {row.status}
    </Badge>
  );
}

function PageLoadingSpinner({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "inline-block animate-spin rounded-full border-muted-foreground/30 border-t-foreground",
        className ?? "size-4 border-2",
      )}
      aria-hidden="true"
    />
  );
}

function getContainingFolderPath(entry: EvidenceDirectoryEntry) {
  if (entry.kind === "directory") {
    return entry.path;
  }

  const normalizedPath = entry.path.replace(/\\/g, "/");
  const lastSeparatorIndex = normalizedPath.lastIndexOf("/");

  if (lastSeparatorIndex <= 0) {
    return entry.path;
  }

  if (/^[A-Za-z]:\//.test(normalizedPath) && lastSeparatorIndex === 2) {
    return entry.path.slice(0, 3);
  }

  return entry.path.slice(0, lastSeparatorIndex);
}

async function openContainingFolder(entry: EvidenceDirectoryEntry) {
  try {
    await openPath(getContainingFolderPath(entry));
  } catch (caughtError) {
    console.error("Failed to open containing folder", caughtError);
  }
}

export function FileListViewer({
  entries,
  isLoading,
  selectedEntry,
  title = "Logical Files",
  statusLabel = "Directory-only acquisition",
  emptyLabel = "No files in this directory",
  canGoBack,
  pageInfo = null,
  isPageLoading = false,
  onGoBack,
  onNextPage,
  onOpenFolder,
  onPreviousPage,
  onRemoveTag,
  onSelectEntry,
  onSetTag,
  tagsByPath = {},
}: FileListViewerProps) {
  const listShellRef = useRef<HTMLElement | null>(null);
  const [tagDialog, setTagDialog] = useState<{
    entry: EvidenceDirectoryEntry;
    mode: "comment" | "new";
  } | null>(null);
  const [tagDialogName, setTagDialogName] = useState("");
  const [tagDialogComment, setTagDialogComment] = useState("");
  const [sort, setSort] = useState<SortState>({
    key: "name",
    direction: "asc",
  });
  const fileRows = useMemo(() => createFileRows(entries), [entries]);
  const sortedRows = useMemo(() => sortFileRows(fileRows, sort), [fileRows, sort]);
  const selectedRowIndex = useMemo(() => {
    if (!selectedEntry) {
      return -1;
    }

    return sortedRows.findIndex((row) => row.id === selectedEntry.id);
  }, [selectedEntry, sortedRows]);

  function focusFileList() {
    listShellRef.current?.focus({ preventScroll: true });
  }

  function selectRowAtIndex(index: number) {
    const boundedIndex = Math.max(0, Math.min(index, sortedRows.length - 1));
    const nextRow = sortedRows[boundedIndex];

    if (nextRow) {
      onSelectEntry(nextRow.entry);
    }
  }

  function handleFileListKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (
      isLoading ||
      sortedRows.length === 0 ||
      isInteractiveEventTarget(event.target)
    ) {
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      selectRowAtIndex(selectedRowIndex < 0 ? 0 : selectedRowIndex + 1);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      selectRowAtIndex(
        selectedRowIndex < 0 ? sortedRows.length - 1 : selectedRowIndex - 1,
      );
      return;
    }

    if (event.key.toLowerCase() === "b" && (event.ctrlKey || event.metaKey)) {
      const selectedRow = selectedRowIndex >= 0 ? sortedRows[selectedRowIndex] : null;

      if (!selectedRow || !onSetTag) {
        return;
      }

      event.preventDefault();
      const selectedRowTags = getTagsForEntry(tagsByPath, selectedRow.entry);

      if (hasTag(selectedRowTags, "Bookmark")) {
        onRemoveTag?.(selectedRow.entry, "Bookmark");
        return;
      }

      onSetTag({
        entry: selectedRow.entry,
        tagName: "Bookmark",
        tagGroup: "bookmark",
      });
    }
  }

  function handleFileListMouseDown(event: MouseEvent<HTMLElement>) {
    if (!isInteractiveEventTarget(event.target)) {
      focusFileList();
    }
  }

  function openTagDialog(
    entry: EvidenceDirectoryEntry,
    mode: "comment" | "new",
  ) {
    onSelectEntry(entry);
    setTagDialog({ entry, mode });
    setTagDialogName(mode === "comment" ? "Comment" : "");
    setTagDialogComment("");
  }

  function submitTagDialog() {
    if (!tagDialog || !onSetTag) {
      return;
    }

    const tagName = tagDialogName.trim();

    if (!tagName) {
      return;
    }

    onSetTag({
      entry: tagDialog.entry,
      tagName,
      tagGroup: "custom",
      comment: tagDialogComment,
    });
    setTagDialog(null);
    setTagDialogName("");
    setTagDialogComment("");
  }

  return (
    <>
      <section
      ref={listShellRef}
      tabIndex={0}
      className="relative flex h-full min-h-0 min-w-0 flex-col overflow-hidden focus:outline-none"
      aria-label="Logical file table"
      onKeyDown={handleFileListKeyDown}
      onMouseDown={handleFileListMouseDown}
    >
      <div className="flex h-8 shrink-0 items-center justify-between gap-2 border-b px-2">
        <div className="flex min-w-0 items-center gap-2">
          <h1 className="text-xs font-medium uppercase text-muted-foreground">
            {title}
          </h1>
          <Button
            type="button"
            variant="outline"
            size="xs"
            className="h-6 rounded-sm px-1.5 text-[11px]"
            disabled={!canGoBack || isLoading}
            onClick={onGoBack}
          >
            <ArrowLeft className="size-3" aria-hidden="true" />
            Back
          </Button>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {pageInfo && (
            <div className="flex items-center gap-1">
              <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
                {formatPageRange(pageInfo, entries.length)}
              </Badge>
              {isPageLoading && (
                <div
                  className="grid size-6 place-items-center"
                  role="status"
                  aria-label="Loading page"
                >
                  <PageLoadingSpinner />
                </div>
              )}
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="size-6 rounded-sm"
                disabled={pageInfo.offset === 0 || isPageLoading}
                onClick={onPreviousPage}
                title="Previous page"
              >
                <ChevronLeft className="size-3" aria-hidden="true" />
                <span className="sr-only">Previous page</span>
              </Button>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="size-6 rounded-sm"
                disabled={!pageInfo.hasNextPage || isPageLoading}
                onClick={onNextPage}
                title="Next page"
              >
                <ChevronRight className="size-3" aria-hidden="true" />
                <span className="sr-only">Next page</span>
              </Button>
            </div>
          )}
          <Badge
            variant="secondary"
            className="h-5 rounded-sm text-[11px]"
          >
            {isPageLoading ? "Loading page" : isLoading ? "Loading" : statusLabel}
          </Badge>
        </div>
      </div>

      {isLoading && (
        <div
          className="absolute inset-0 z-30 grid place-items-center bg-background/70 backdrop-blur-[1px]"
          role="status"
          aria-label="Loading file list"
        >
          <div className="flex flex-col items-center gap-3 rounded-sm border bg-background/95 px-5 py-4 text-xs text-muted-foreground shadow-md">
            <PageLoadingSpinner className="size-12 border-4" />
            <span>{isPageLoading ? "Loading page" : "Loading file list"}</span>
          </div>
        </div>
      )}

      <div className="relative min-h-0 flex-1 overflow-auto">
        <div className="flex h-full min-w-[1198px] flex-col text-xs">
          <div
            className="sticky top-0 z-10 grid h-7 border-b bg-muted"
            style={{ gridTemplateColumns: fileListGridTemplateColumns }}
          >
            {columns.map((column) => (
              <div
                key={column.key}
                className={cn("min-w-0 px-2", column.className)}
              >
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  className="h-6 w-full justify-start gap-1 rounded-sm px-1 text-[11px] font-medium uppercase text-muted-foreground"
                  onClick={() => {
                    setSort((current) => getNextSort(current, column.key));
                  }}
                >
                  <span className="truncate">{column.label}</span>
                  <SortIcon columnKey={column.key} sort={sort} />
                </Button>
              </div>
            ))}
          </div>
          {sortedRows.length === 0 && (
            <div className="grid h-20 place-items-center text-center text-xs text-muted-foreground">
              {isLoading ? "Loading" : emptyLabel}
            </div>
          )}
          {sortedRows.length > 0 && (
            <div className="min-h-0 flex-1">
              <AutoSizer>
                {({ height, width }) => (
                  <List
                    height={height}
                    rowCount={sortedRows.length}
                    rowHeight={32}
                    rowRenderer={(props: ListRowProps) => {
                      const row = sortedRows[props.index];

                      return (
                        <FileListRow
                          key={props.key}
                          row={row}
                          selectedEntry={selectedEntry}
                          style={props.style}
                          onFocusList={focusFileList}
                          onOpenFolder={onOpenFolder}
                          onOpenTagDialog={openTagDialog}
                          onRemoveTag={onRemoveTag}
                          onSelectEntry={onSelectEntry}
                          onSetTag={onSetTag}
                          tags={getTagsForEntry(tagsByPath, row.entry)}
                        />
                      );
                    }}
                    overscanRowCount={12}
                    scrollToAlignment="auto"
                    scrollToIndex={
                      selectedRowIndex >= 0 ? selectedRowIndex : undefined
                    }
                    width={Math.max(width, 1198)}
                  />
                )}
              </AutoSizer>
            </div>
          )}
        </div>
      </div>
      </section>

      <Dialog
        open={Boolean(tagDialog)}
        onOpenChange={(isOpen) => {
          if (!isOpen) {
            setTagDialog(null);
          }
        }}
      >
        <DialogContent className="w-[min(28rem,calc(100vw-2rem))] rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">
              {tagDialog?.mode === "new" ? "New Tag" : "Tag and Comment"}
            </DialogTitle>
            <DialogDescription className="text-xs">
              {tagDialog?.entry.name ?? "Selected item"}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-2 p-3 text-xs">
            <label className="grid gap-1">
              <span className="text-muted-foreground">Tag</span>
              <Input
                className="h-8 rounded-sm text-xs"
                value={tagDialogName}
                onChange={(event) => setTagDialogName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    submitTagDialog();
                  }
                }}
              />
            </label>
            <label className="grid gap-1">
              <span className="text-muted-foreground">Comment</span>
              <Input
                className="h-8 rounded-sm text-xs"
                value={tagDialogComment}
                onChange={(event) => setTagDialogComment(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    submitTagDialog();
                  }
                }}
              />
            </label>
          </div>
          <DialogFooter className="border-t p-2">
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              onClick={() => setTagDialog(null)}
            >
              Cancel
            </Button>
            <Button
              type="button"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              disabled={!tagDialogName.trim() || !onSetTag}
              onClick={submitTagDialog}
            >
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

function FileListRow({
  row,
  selectedEntry,
  style,
  onFocusList,
  onOpenFolder,
  onOpenTagDialog,
  onRemoveTag,
  onSelectEntry,
  onSetTag,
  tags,
}: {
  row: FileRow;
  selectedEntry: EvidenceDirectoryEntry | null;
  style: CSSProperties;
  onFocusList: () => void;
  onOpenFolder: (entry: EvidenceDirectoryEntry) => void;
  onOpenTagDialog: (
    entry: EvidenceDirectoryEntry,
    mode: "comment" | "new",
  ) => void;
  onRemoveTag?: (entry: EvidenceDirectoryEntry, tagName: string) => void;
  onSelectEntry: (entry: EvidenceDirectoryEntry) => void;
  onSetTag?: (input: {
    entry: EvidenceDirectoryEntry;
    tagName: string;
    tagGroup: FileTagGroup;
    comment?: string;
  }) => void;
  tags: FileTagRecord[];
}) {
  const sizeLabel = getEntrySizeLabel(row.entry);
  const modifiedLabel = formatModifiedTime(row.entry.modifiedMs);

  function selectRowForTagAction() {
    onFocusList();
    onSelectEntry(row.entry);
  }

  function toggleTag(tagName: string, tagGroup: FileTagGroup, checked: boolean) {
    selectRowForTagAction();

    if (checked) {
      onSetTag?.({ entry: row.entry, tagName, tagGroup });
      return;
    }

    onRemoveTag?.(row.entry, tagName);
  }

  function openDialogTag(mode: "comment" | "new") {
    selectRowForTagAction();
    onOpenTagDialog(row.entry, mode);
  }

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          data-state={selectedEntry?.id === row.id ? "selected" : undefined}
          className={cn(
            "grid h-8 cursor-default items-center border-b text-xs hover:bg-muted/50 data-[state=selected]:bg-accent",
          )}
          style={{
            ...style,
            gridTemplateColumns: fileListGridTemplateColumns,
          }}
          onClick={() => {
            onFocusList();
            onSelectEntry(row.entry);
          }}
          onContextMenu={() => {
            onFocusList();
            onSelectEntry(row.entry);
          }}
          onDoubleClick={() => {
            if (row.entry.kind === "directory") {
              onOpenFolder(row.entry);
            }
          }}
        >
          <div className="min-w-0 px-2">
            <FileNameCell row={row} tags={tags} />
          </div>
          <div className="truncate px-2 text-muted-foreground">{row.path}</div>
          <div className="truncate px-2">{row.type}</div>
          <div className="truncate px-2">{sizeLabel}</div>
          <div className="truncate px-2">{modifiedLabel}</div>
          <div className="truncate px-2 text-muted-foreground">{row.plugin}</div>
          <div className="px-2">
            <StatusBadge row={row} />
          </div>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent className="w-48">
        {row.entry.kind === "directory" ? (
          <ContextMenuItem
            className="text-xs"
            onSelect={() => {
              onOpenFolder(row.entry);
            }}
          >
            <FolderOpen className="size-3.5" aria-hidden="true" />
            Open
          </ContextMenuItem>
        ) : (
          <ContextMenuItem
            className="text-xs"
            onSelect={() => {
              void openContainingFolder(row.entry);
            }}
          >
            <FolderOpen className="size-3.5" aria-hidden="true" />
            Open Containing Folder
          </ContextMenuItem>
        )}
        <ContextMenuSeparator />
        <ContextMenuSub>
          <ContextMenuSubTrigger className="text-xs">
            <Tag className="size-3.5" aria-hidden="true" />
            Tag
          </ContextMenuSubTrigger>
          <ContextMenuSubContent className="w-56">
            <ContextMenuCheckboxItem
              className="text-xs"
              checked={hasTag(tags, "Bookmark")}
              onCheckedChange={(checked) => {
                toggleTag("Bookmark", "bookmark", checked === true);
              }}
            >
              <Bookmark className="size-3.5" aria-hidden="true" />
              Bookmark
              <ContextMenuShortcut>Ctrl+B</ContextMenuShortcut>
            </ContextMenuCheckboxItem>
            <ContextMenuCheckboxItem
              className="text-xs"
              checked={hasTag(tags, "Follow Up")}
              onCheckedChange={(checked) => {
                toggleTag("Follow Up", "follow-up", checked === true);
              }}
            >
              <Flag className="size-3.5" aria-hidden="true" />
              Follow Up
            </ContextMenuCheckboxItem>
            <ContextMenuCheckboxItem
              className="text-xs"
              checked={hasTag(tags, "Notable Item (Notable)")}
              onCheckedChange={(checked) => {
                toggleTag(
                  "Notable Item (Notable)",
                  "notable",
                  checked === true,
                );
              }}
            >
              <Tag className="size-3.5" aria-hidden="true" />
              Notable Item (Notable)
            </ContextMenuCheckboxItem>
            <ContextMenuSub>
              <ContextMenuSubTrigger className="text-xs">
                <Tag className="size-3.5" aria-hidden="true" />
                Project VIC
              </ContextMenuSubTrigger>
              <ContextMenuSubContent className="w-44">
                {[0, 1, 2, 3, 4].map((categoryNumber) => {
                  const tagName = `Project VIC: Category ${categoryNumber}`;

                  return (
                    <ContextMenuCheckboxItem
                      key={tagName}
                      className="text-xs"
                      checked={hasTag(tags, tagName)}
                      onCheckedChange={(checked) => {
                        toggleTag(tagName, "project-vic", checked === true);
                      }}
                    >
                      Category {categoryNumber}
                    </ContextMenuCheckboxItem>
                  );
                })}
              </ContextMenuSubContent>
            </ContextMenuSub>
            <ContextMenuSeparator />
            <ContextMenuItem
              className="text-xs"
              onSelect={() => openDialogTag("comment")}
            >
              <MessageSquarePlus className="size-3.5" aria-hidden="true" />
              Tag and Comment...
            </ContextMenuItem>
            <ContextMenuItem
              className="text-xs"
              onSelect={() => openDialogTag("new")}
            >
              <Plus className="size-3.5" aria-hidden="true" />
              New Tag...
            </ContextMenuItem>
          </ContextMenuSubContent>
        </ContextMenuSub>
      </ContextMenuContent>
    </ContextMenu>
  );
}
