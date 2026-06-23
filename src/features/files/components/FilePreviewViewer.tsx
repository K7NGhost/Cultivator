import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import {
  AutoSizer,
  CellMeasurer,
  CellMeasurerCache,
  List,
  type ListRowProps,
} from "react-virtualized";
import "react-virtualized/styles.css";
import { ChevronLeft, ChevronRight, Database, FileCode2, Hexagon } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
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
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";
import { cn } from "@/lib/utils";

type FilePreviewViewerProps = {
  activeTab: FilePreviewTab;
  filePreview: FileFormatPreview | null;
  hexPreview: string[];
  isLoading: boolean;
  onActiveTabChange: (tab: FilePreviewTab) => void;
  selectedEntry: EvidenceDirectoryEntry | null;
  textPreview: string[];
};

export type FilePreviewTab = "file" | "text" | "hex";

export type FileFormatPreview = {
  kind: string;
  label: string;
  mediaPath?: string | null;
  details: Array<{
    label: string;
    value: string;
  }>;
};

type SqliteTableSummary = {
  name: string;
  tableType: string;
  rowCount: number;
};

type SqliteTableRows = {
  columns: string[];
  rows: unknown[][];
  totalRows: number;
};

type PreviewLinesListProps = {
  emptyText: string;
  isLoading: boolean;
  lines: string[];
  wordWrap: boolean;
};

type ParsedPreviewLine = {
  content: string;
  lineNumber: string | null;
};

const TEXT_WORD_WRAP_STORAGE_KEY = "cultivator.files.textPreview.wordWrap";
const TEXT_ROW_HEIGHT = 20;
const TEXT_CHARACTER_WIDTH = 8;
const TEXT_GUTTER_WIDTH = 64;

function parsePreviewLine(line: string): ParsedPreviewLine {
  const match = line.match(/^(\s*\d+)\s{2}(.*)$/s);

  if (!match) {
    return {
      content: line,
      lineNumber: null,
    };
  }

  return {
    content: match[2] ?? "",
    lineNumber: match[1]?.trim() ?? null,
  };
}

function loadTextWordWrapSetting() {
  if (typeof localStorage === "undefined") {
    return true;
  }

  return localStorage.getItem(TEXT_WORD_WRAP_STORAGE_KEY) !== "false";
}

function saveTextWordWrapSetting(isEnabled: boolean) {
  if (typeof localStorage === "undefined") {
    return;
  }

  localStorage.setItem(TEXT_WORD_WRAP_STORAGE_KEY, String(isEnabled));
}

function PreviewLinesList({
  emptyText,
  isLoading,
  lines,
  wordWrap,
}: PreviewLinesListProps) {
  const measurementCache = useMemo(
    () =>
      new CellMeasurerCache({
        defaultHeight: 20,
        fixedWidth: true,
        minHeight: 20,
      }),
    [lines, wordWrap],
  );

  if (lines.length === 0) {
    return (
      <div className="p-2 font-mono text-xs text-muted-foreground">
        {isLoading ? "Loading preview..." : emptyText}
      </div>
    );
  }

  if (!wordWrap) {
    return <UnwrappedLinesList lines={lines} />;
  }

  return (
    <AutoSizer>
      {({ height, width }) => (
        <List
          className="font-mono text-xs"
          width={width}
          height={height}
          rowCount={lines.length}
          rowHeight={measurementCache.rowHeight}
          deferredMeasurementCache={measurementCache}
          overscanRowCount={20}
          rowRenderer={({ index, key, parent, style }: ListRowProps) => {
            const parsedLine = parsePreviewLine(lines[index]);

            return (
              <CellMeasurer
                cache={measurementCache}
                columnIndex={0}
                key={key}
                parent={parent}
                rowIndex={index}
              >
                {({ registerChild }) => (
                  <div
                    ref={registerChild}
                    style={style}
                    className="grid grid-cols-[4rem_minmax(0,1fr)] leading-5"
                  >
                    <div className="select-none border-r px-1 text-right text-muted-foreground">
                      {parsedLine.lineNumber ?? ""}
                    </div>
                    <div className="min-w-0 whitespace-pre-wrap break-words px-2">
                      {parsedLine.content}
                    </div>
                  </div>
                )}
              </CellMeasurer>
            );
          }}
        />
      )}
    </AutoSizer>
  );
}

function UnwrappedLinesList({ lines }: { lines: string[] }) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const contentWidth = useMemo(() => {
    const longestLineLength = lines.reduce((longest, line) => {
      return Math.max(longest, parsePreviewLine(line).content.length);
    }, 0);

    return longestLineLength * TEXT_CHARACTER_WIDTH + TEXT_GUTTER_WIDTH + 16;
  }, [lines]);
  const startIndex = Math.max(0, Math.floor(scrollTop / TEXT_ROW_HEIGHT) - 8);
  const visibleRowCount = Math.ceil(viewportHeight / TEXT_ROW_HEIGHT) + 16;
  const endIndex = Math.min(lines.length, startIndex + visibleRowCount);
  const visibleLines = lines.slice(startIndex, endIndex);

  useLayoutEffect(() => {
    const element = scrollRef.current;

    if (!element) {
      return;
    }

    const resizeObserver = new ResizeObserver(([entry]) => {
      setViewportHeight(entry.contentRect.height);
    });

    resizeObserver.observe(element);
    setViewportHeight(element.clientHeight);

    return () => resizeObserver.disconnect();
  }, []);

  return (
    <div
      ref={scrollRef}
      className="h-full min-h-0 min-w-0 overflow-auto font-mono text-xs"
      onScroll={(event) => {
        setScrollTop(event.currentTarget.scrollTop);
      }}
    >
      <div
        className="relative"
        style={{
          height: `${lines.length * TEXT_ROW_HEIGHT}px`,
          minWidth: "100%",
          width: `${contentWidth}px`,
        }}
      >
        {visibleLines.map((line, index) => {
          const lineIndex = startIndex + index;
          const parsedLine = parsePreviewLine(line);

          return (
            <div
              key={lineIndex}
              className="absolute left-0 right-0 grid grid-cols-[4rem_minmax(0,1fr)] whitespace-pre leading-5"
              style={{
                height: `${TEXT_ROW_HEIGHT}px`,
                top: `${lineIndex * TEXT_ROW_HEIGHT}px`,
              }}
            >
              <div className="select-none border-r px-1 text-right text-muted-foreground">
                {parsedLine.lineNumber ?? ""}
              </div>
              <div className="px-2">{parsedLine.content}</div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function HexLinesList({
  emptyText,
  isLoading,
  lines,
}: Omit<PreviewLinesListProps, "wordWrap">) {
  if (lines.length === 0) {
    return (
      <div className="p-2 font-mono text-xs text-muted-foreground">
        {isLoading ? "Loading preview..." : emptyText}
      </div>
    );
  }

  return (
    <AutoSizer>
      {({ height, width }) => (
        <List
          className="font-mono text-xs"
          width={width}
          height={height}
          rowCount={lines.length}
          rowHeight={20}
          overscanRowCount={20}
          rowRenderer={({ index, key, style }: ListRowProps) => (
            <div
              key={key}
              style={style}
              className="overflow-hidden whitespace-pre px-2 leading-5"
            >
              {lines[index]}
            </div>
          )}
        />
      )}
    </AutoSizer>
  );
}

function FileFormatPanel({
  filePreview,
  isLoading,
  selectedEntry,
}: {
  filePreview: FileFormatPreview | null;
  isLoading: boolean;
  selectedEntry: EvidenceDirectoryEntry | null;
}) {
  if (isLoading) {
    return <div className="p-2 text-xs text-muted-foreground">Loading file viewer...</div>;
  }

  if (!filePreview) {
    return (
      <div className="p-2 text-xs text-muted-foreground">
        {selectedEntry?.kind === "directory"
          ? "Select a file to inspect its format."
          : "No format-specific viewer is available for this file."}
      </div>
    );
  }

  if (filePreview.kind === "sqlite" && selectedEntry?.kind === "file") {
    return <SqliteDatabaseViewer path={selectedEntry.path} metadata={filePreview} />;
  }

  if (filePreview.mediaPath) {
    return <ImageFileViewer filePreview={filePreview} />;
  }

  return (
    <div className="h-full min-h-0 overflow-auto p-2 text-xs">
      <div className="mb-2 flex h-7 items-center gap-2 border-b pb-2">
        <Database className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <div className="font-medium">{filePreview.label}</div>
        <div className="rounded-sm border px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground">
          {filePreview.kind}
        </div>
      </div>
      <Table containerClassName="rounded-sm border" className="text-xs">
        <TableBody>
          {filePreview.details.map((detail) => (
            <TableRow key={detail.label} className="h-7">
              <TableCell className="w-40 border-r px-2 py-1 text-muted-foreground">
                {detail.label}
              </TableCell>
              <TableCell className="px-2 py-1 font-mono text-[11px]">
                {detail.value}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

function ImageFileViewer({ filePreview }: { filePreview: FileFormatPreview }) {
  const imageSource = filePreview.mediaPath
    ? convertFileSrc(filePreview.mediaPath)
    : "";

  return (
    <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_13rem] text-[11px]">
      <div className="grid min-h-0 place-items-center overflow-auto bg-muted/20 p-1.5">
        {imageSource ? (
          <img
            src={imageSource}
            alt={filePreview.label}
            className="max-h-full max-w-full object-contain"
            draggable={false}
          />
        ) : (
          <div className="text-muted-foreground">No image preview available.</div>
        )}
      </div>
      <aside className="min-h-0 overflow-auto border-l">
        <div className="flex h-6 items-center gap-1.5 border-b px-1.5">
          <Database className="size-3 text-muted-foreground" aria-hidden="true" />
          <span className="truncate font-medium">{filePreview.label}</span>
        </div>
        <Table containerClassName="border-0" className="text-[11px]">
          <TableBody>
            {filePreview.details.map((detail) => (
              <TableRow key={detail.label} className="h-6">
                <TableCell className="w-20 border-r px-1.5 py-0.5 text-muted-foreground">
                  {detail.label}
                </TableCell>
                <TableCell className="px-1.5 py-0.5 font-mono text-[10px]">
                  {detail.value}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </aside>
    </div>
  );
}

function SqliteDatabaseViewer({
  path,
  metadata,
}: {
  path: string;
  metadata: FileFormatPreview;
}) {
  const [tables, setTables] = useState<SqliteTableSummary[]>([]);
  const [selectedTable, setSelectedTable] = useState("");
  const [tableRows, setTableRows] = useState<SqliteTableRows | null>(null);
  const [offset, setOffset] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [isTablesLoading, setIsTablesLoading] = useState(false);
  const [isRowsLoading, setIsRowsLoading] = useState(false);
  const pageSize = 100;

  useEffect(() => {
    let isCurrent = true;

    setIsTablesLoading(true);
    setError(null);
    setTables([]);
    setSelectedTable("");
    setTableRows(null);
    setOffset(0);

    invoke<SqliteTableSummary[]>("list_sqlite_tables", { path })
      .then((nextTables) => {
        if (!isCurrent) {
          return;
        }

        setTables(nextTables);
        setSelectedTable(nextTables[0]?.name ?? "");
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
      })
      .finally(() => {
        if (isCurrent) {
          setIsTablesLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [path]);

  useEffect(() => {
    if (!selectedTable) {
      setTableRows(null);
      return;
    }

    let isCurrent = true;

    setIsRowsLoading(true);
    setError(null);

    invoke<SqliteTableRows>("read_sqlite_table_rows", {
      path,
      table: selectedTable,
      limit: pageSize,
      offset,
    })
      .then((nextRows) => {
        if (isCurrent) {
          setTableRows(nextRows);
        }
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
        setTableRows(null);
      })
      .finally(() => {
        if (isCurrent) {
          setIsRowsLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [offset, path, selectedTable]);

  const selectedSummary = tables.find((table) => table.name === selectedTable);
  const canGoPrevious = offset > 0;
  const canGoNext = Boolean(tableRows && offset + pageSize < tableRows.totalRows);

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b bg-muted/20 px-2 text-xs">
        <Database className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <span className="font-medium">{metadata.label}</span>
        <span className="text-muted-foreground">{tables.length} tables</span>
        {error && <span className="truncate text-destructive">{error}</span>}
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[15rem_minmax(0,1fr)]">
        <aside className="min-h-0 border-r">
          <div className="flex h-7 items-center border-b px-2 text-[11px] font-medium uppercase text-muted-foreground">
            Tables
          </div>
          <div className="h-[calc(100%-1.75rem)] overflow-auto p-1">
            {tables.map((table) => (
              <button
                key={table.name}
                type="button"
                className={cn(
                  "flex h-7 w-full items-center justify-between gap-2 rounded-sm px-2 text-left text-xs hover:bg-accent",
                  selectedTable === table.name && "bg-accent",
                )}
                onClick={() => {
                  setSelectedTable(table.name);
                  setOffset(0);
                }}
              >
                <span className="min-w-0 truncate">{table.name}</span>
                <span className="shrink-0 text-[11px] text-muted-foreground">
                  {table.rowCount.toLocaleString()}
                </span>
              </button>
            ))}
            {tables.length === 0 && (
              <div className="px-2 py-2 text-xs text-muted-foreground">
                {isTablesLoading ? "Loading tables..." : "No user tables found."}
              </div>
            )}
          </div>
        </aside>

        <section className="flex min-h-0 min-w-0 flex-col">
          <div className="flex h-8 shrink-0 items-center justify-between gap-2 border-b px-2 text-xs">
            <div className="min-w-0 truncate">
              {selectedSummary
                ? `${selectedSummary.name} (${selectedSummary.tableType})`
                : "Select a table"}
            </div>
            <div className="flex items-center gap-1">
              <span className="text-[11px] text-muted-foreground">
                {tableRows
                  ? `${offset + 1}-${Math.min(
                      offset + pageSize,
                      tableRows.totalRows,
                    )} of ${tableRows.totalRows.toLocaleString()}`
                  : "0 rows"}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                disabled={!canGoPrevious || isRowsLoading}
                onClick={() => setOffset((current) => Math.max(0, current - pageSize))}
              >
                <ChevronLeft className="size-3.5" aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                disabled={!canGoNext || isRowsLoading}
                onClick={() => setOffset((current) => current + pageSize)}
              >
                <ChevronRight className="size-3.5" aria-hidden="true" />
              </Button>
            </div>
          </div>

          <SqliteRowsTable tableRows={tableRows} isLoading={isRowsLoading} />
        </section>
      </div>
    </div>
  );
}

function SqliteRowsTable({
  tableRows,
  isLoading,
}: {
  tableRows: SqliteTableRows | null;
  isLoading: boolean;
}) {
  if (!tableRows) {
    return (
      <div className="p-2 text-xs text-muted-foreground">
        {isLoading ? "Loading rows..." : "Select a table to browse rows."}
      </div>
    );
  }

  if (tableRows.columns.length === 0) {
    return (
      <div className="p-2 text-xs text-muted-foreground">
        {isLoading ? "Loading rows..." : "This table has no visible columns."}
      </div>
    );
  }

  return (
    <Table
      containerClassName="min-h-0 flex-1 overflow-auto"
      className="min-w-full table-fixed text-xs"
    >
      <TableHeader className="sticky top-0 z-10 bg-background">
        <TableRow className="h-7 hover:bg-transparent">
          {tableRows.columns.map((column) => (
            <TableHead
              key={column}
              className="h-7 w-48 border-r px-2 py-1 text-[11px] last:border-r-0"
              title={column}
            >
              <span className="block truncate">{column}</span>
            </TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {tableRows.rows.map((row, rowIndex) => (
          <TableRow key={rowIndex} className="h-7">
            {tableRows.columns.map((column, columnIndex) => {
              const value = formatSqliteCell(row[columnIndex]);

              return (
                <TableCell
                  key={column}
                  className="border-r px-2 py-1 text-[11px] last:border-r-0"
                  title={value}
                >
                  <span className="block truncate">{value || "NULL"}</span>
                </TableCell>
              );
            })}
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function formatSqliteCell(value: unknown) {
  if (value === null || value === undefined) {
    return "";
  }

  if (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return String(value);
  }

  return JSON.stringify(value);
}

export function FilePreviewViewer({
  activeTab,
  filePreview,
  hexPreview,
  isLoading,
  onActiveTabChange,
  selectedEntry,
  textPreview,
}: FilePreviewViewerProps) {
  const [isTextWordWrapEnabled, setIsTextWordWrapEnabled] = useState(
    loadTextWordWrapSetting,
  );

  useEffect(() => {
    saveTextWordWrapSetting(isTextWordWrapEnabled);
  }, [isTextWordWrapEnabled]);

  return (
    <section
      className="h-full min-h-0 min-w-0 overflow-hidden"
      aria-label="File preview"
    >
      <Tabs
        value={activeTab}
        onValueChange={(value) => {
          onActiveTabChange(value as FilePreviewTab);
        }}
        className="flex h-full min-h-0 min-w-0 flex-col gap-0"
      >
        <div className="flex h-8 items-center justify-between border-b px-2">
          <div className="flex min-w-0 items-center gap-2 text-xs">
            <span className="font-medium">Preview:</span>
            <span className="truncate text-muted-foreground">
              {selectedEntry?.path ?? "No file selected"}
            </span>
          </div>
          <TabsList
            variant="line"
            className="h-7 rounded-none p-0"
            aria-label="Preview mode"
          >
            <TabsTrigger
              value="text"
              className="h-7 rounded-none px-2 text-xs"
            >
              <FileCode2 className="size-3.5" aria-hidden="true" />
              Text
            </TabsTrigger>
            <TabsTrigger
              value="file"
              className="h-7 rounded-none px-2 text-xs"
            >
              <Database className="size-3.5" aria-hidden="true" />
              File
            </TabsTrigger>
            <TabsTrigger value="hex" className="h-7 rounded-none px-2 text-xs">
              <Hexagon className="size-3.5" aria-hidden="true" />
              Hex
            </TabsTrigger>
          </TabsList>
        </div>

        <TabsContent
          value="text"
          className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <ContextMenu>
            <ContextMenuTrigger className="block h-full min-h-0 min-w-0">
              <PreviewLinesList
                lines={textPreview}
                isLoading={isLoading}
                wordWrap={isTextWordWrapEnabled}
                emptyText={
                  selectedEntry?.kind === "directory"
                    ? "Select a file to preview text."
                    : "No text preview available."
                }
              />
            </ContextMenuTrigger>
            <ContextMenuContent className="w-44">
              <ContextMenuCheckboxItem
                checked={isTextWordWrapEnabled}
                onCheckedChange={(checked) => {
                  setIsTextWordWrapEnabled(checked === true);
                }}
              >
                Word Wrap
              </ContextMenuCheckboxItem>
            </ContextMenuContent>
          </ContextMenu>
        </TabsContent>

        <TabsContent
          value="file"
          className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <FileFormatPanel
            filePreview={filePreview}
            isLoading={isLoading}
            selectedEntry={selectedEntry}
          />
        </TabsContent>

        <TabsContent
          value="hex"
          className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <HexLinesList
            lines={hexPreview}
            isLoading={isLoading}
            emptyText={
              selectedEntry?.kind === "directory"
                ? "Select a file to preview hex."
                : "No hex preview available."
            }
          />
        </TabsContent>
      </Tabs>
    </section>
  );
}
