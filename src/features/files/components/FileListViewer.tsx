import { useMemo, useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  ArrowUpDown,
  File,
  FileCode2,
  FileImage,
  FolderOpen,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";
import { cn } from "@/lib/utils";

type FileListViewerProps = {
  entries: EvidenceDirectoryEntry[];
  isLoading: boolean;
  selectedEntry: EvidenceDirectoryEntry | null;
  canGoBack: boolean;
  onGoBack: () => void;
  onOpenFolder: (entry: EvidenceDirectoryEntry) => void;
  onSelectEntry: (entry: EvidenceDirectoryEntry) => void;
};

type FileRow = {
  entry: EvidenceDirectoryEntry;
  id: string;
  name: string;
  path: string;
  type: string;
  sizeLabel: string;
  sizeValue: number;
  modifiedLabel: string;
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

  return extension && extension !== entry.name
    ? extension.toUpperCase()
    : "File";
}

function getEntryPlugin(entry: EvidenceDirectoryEntry) {
  if (entry.kind === "directory") {
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
    sizeLabel:
      entry.kind === "directory"
        ? `${entry.childCount ?? 0} items`
        : formatFileSize(entry.size),
    sizeValue: entry.kind === "directory" ? 0 : (entry.size ?? 0),
    modifiedLabel: formatModifiedTime(entry.modifiedMs),
    modifiedValue: entry.modifiedMs ?? 0,
    plugin: getEntryPlugin(entry),
    status: entry.kind === "directory" ? "Folder" : "Indexed",
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

function FileNameCell({ row }: { row: FileRow }) {
  const Icon = getEntryIcon(row.entry);

  return (
    <div className="flex min-w-0 items-center gap-1.5">
      <Icon
        className={cn("size-3.5 shrink-0", getEntryIconClassName(row.entry))}
        aria-hidden="true"
      />
      <span className="min-w-0 truncate font-medium">{row.name}</span>
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
  canGoBack,
  onGoBack,
  onOpenFolder,
  onSelectEntry,
}: FileListViewerProps) {
  const [sort, setSort] = useState<SortState>({
    key: "name",
    direction: "asc",
  });
  const fileRows = useMemo(() => createFileRows(entries), [entries]);
  const sortedRows = useMemo(() => sortFileRows(fileRows, sort), [fileRows, sort]);

  return (
    <section
      className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden"
      aria-label="Logical file table"
    >
      <div className="flex h-8 shrink-0 items-center justify-between gap-2 border-b px-2">
        <div className="flex min-w-0 items-center gap-2">
          <h1 className="text-xs font-medium uppercase text-muted-foreground">
            Logical Files
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
        <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
          {isLoading ? "Loading folder" : "Directory-only acquisition"}
        </Badge>
      </div>

      <Table
        containerClassName="min-h-0 flex-1 overflow-auto"
        className="min-w-[1198px] table-fixed text-xs"
      >
        <TableHeader className="sticky top-0 z-10 bg-muted">
          <TableRow className="hover:bg-muted">
            {columns.map((column) => (
              <TableHead
                key={column.key}
                className={cn("h-7 px-2", column.className)}
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
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {sortedRows.map((row) => (
            <ContextMenu key={row.id}>
              <ContextMenuTrigger asChild>
                <TableRow
                  data-state={
                    selectedEntry?.id === row.id ? "selected" : undefined
                  }
                  className="h-8 cursor-default"
                  onClick={() => {
                    onSelectEntry(row.entry);
                  }}
                  onContextMenu={() => {
                    onSelectEntry(row.entry);
                  }}
                  onDoubleClick={() => {
                    if (row.entry.kind === "directory") {
                      onOpenFolder(row.entry);
                    }
                  }}
                >
                  <TableCell className="h-8 p-0">
                    <FileNameCell row={row} />
                  </TableCell>
                  <TableCell className="h-8 truncate p-0 text-muted-foreground">
                    {row.path}
                  </TableCell>
                  <TableCell className="h-8 p-0">{row.type}</TableCell>
                  <TableCell className="h-8 p-0">{row.sizeLabel}</TableCell>
                  <TableCell className="h-8 p-0">
                    {row.modifiedLabel}
                  </TableCell>
                  <TableCell className="h-8 truncate p-0 text-muted-foreground">
                    {row.plugin}
                  </TableCell>
                  <TableCell className="h-8 p-0">
                    <StatusBadge row={row} />
                  </TableCell>
                </TableRow>
              </ContextMenuTrigger>
              <ContextMenuContent className="w-48">
                <ContextMenuItem
                  className="text-xs"
                  onSelect={() => {
                    void openContainingFolder(row.entry);
                  }}
                >
                  <FolderOpen className="size-3.5" aria-hidden="true" />
                  Open Containing Folder
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
          ))}
          {sortedRows.length === 0 && (
            <TableRow>
              <TableCell
                colSpan={columns.length}
                className="h-20 text-center text-xs text-muted-foreground"
              >
                {isLoading ? "Loading folder" : "No files in this directory"}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </section>
  );
}
