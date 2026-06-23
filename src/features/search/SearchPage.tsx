import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  AutoSizer,
  CellMeasurer,
  CellMeasurerCache,
  List,
  type ListRowProps,
} from "react-virtualized";
import "react-virtualized/styles.css";
import {
  AlertCircle,
  CaseSensitive,
  ChevronLeft,
  ChevronRight,
  FileCode2,
  FileText,
  FolderOpen,
  Hexagon,
  Play,
  Regex,
  Search,
  SlidersHorizontal,
  Square,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useCases } from "@/features/cases/case-provider";
import {
  listDataSources,
  subscribeToDataSourcesChanged,
} from "@/features/datasources/dataSourceRepository";
import type { DataSourceRecord } from "@/features/datasources/types";
import { useEvidence } from "@/features/evidence/evidence-provider";
import { cn } from "@/lib/utils";
import {
  buildSearchReport,
  type SearchReportStatus,
} from "./searchReport";
import {
  loadSearchSettings,
  saveSearchSettings,
} from "./searchSettings";

type SearchMatch = {
  id: string;
  file: string;
  path: string;
  line: number;
  column: number;
  kind: string;
  matchedText: string;
  context: string;
};

type SearchResult = {
  matches: SearchMatch[];
  elapsedMs: number;
  cancelled: boolean;
  scannedFiles: number;
  totalFiles: number;
  totalComplete: boolean;
};

type SearchProgress = {
  searchId: string;
  scannedFiles: number;
  totalFiles: number;
  totalComplete: boolean;
  elapsedMs: number;
};

type SearchFileSummary = {
  path: string;
  file: string;
  kind: string;
  matchCount: number;
  matchedLines: number[];
  firstMatch: SearchMatch;
};

type SearchSummaries = {
  searchId: string;
  files: SearchFileSummary[];
  elapsedMs: number;
};

type SearchScope = {
  id: string;
  name: string;
  description: string;
  paths: string[];
};

type FileMatch = {
  match: SearchMatch;
  matchCount: number;
  matchedLines: Set<number>;
};

type ParsedPreviewLine = {
  content: string;
  lineNumber: number | null;
};

type TextPreviewListProps = {
  activePreviewMatch: SearchMatch | null;
  currentPreviewMatchLines: Set<number>;
  height: number;
  lines: string[];
  selectedTextLineIndex: number;
  width: number;
};

type SearchFileMatchListProps = {
  fileMatches: FileMatch[];
  isSearching: boolean;
  onSelectMatch: (match: SearchMatch) => void;
  selectedPath: string | null;
};

const SEARCH_UI_MATCH_FLUSH_INTERVAL_MS = 100;
const FILE_MATCH_HEADER_HEIGHT = 28;
const FILE_MATCH_ROW_HEIGHT = 32;
const FILE_MATCH_GRID_COLUMNS = "minmax(0,1fr) 4.75rem 4.25rem 4.5rem";

function parsePreviewLine(line: string): ParsedPreviewLine {
  const match = line.match(/^(\s*\d+)(?::|\s)(.*)$/s);

  if (!match) {
    return {
      content: line,
      lineNumber: null,
    };
  }

  return {
    content: match[2] ?? "",
    lineNumber: Number.parseInt(match[1], 10),
  };
}

function TextPreviewList({
  activePreviewMatch,
  currentPreviewMatchLines,
  height,
  lines,
  selectedTextLineIndex,
  width,
}: TextPreviewListProps) {
  const list = useRef<List | null>(null);
  const measurementCache = useMemo(
    () =>
      new CellMeasurerCache({
        defaultHeight: 20,
        fixedWidth: true,
        minHeight: 20,
      }),
    [lines, width],
  );

  useEffect(() => {
    list.current?.scrollToRow(selectedTextLineIndex);
  }, [selectedTextLineIndex, lines.length]);

  return (
    <List
      ref={(nextList) => {
        list.current = nextList;
      }}
      className="font-mono text-xs"
      width={width}
      height={height}
      rowCount={lines.length}
      rowHeight={measurementCache.rowHeight}
      deferredMeasurementCache={measurementCache}
      overscanRowCount={12}
      scrollToAlignment="center"
      scrollToIndex={selectedTextLineIndex}
      rowRenderer={({ index, key, parent, style }: ListRowProps) => {
        const line = lines[index];
        const { content, lineNumber: previewLineNumber } =
          parsePreviewLine(line);
        const isSelectedLine = activePreviewMatch?.line === previewLineNumber;
        const isMatchedLine =
          previewLineNumber !== null &&
          currentPreviewMatchLines.has(previewLineNumber);

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
                className={cn(
                  "grid grid-cols-[4.5rem_minmax(0,1fr)] leading-5",
                  isMatchedLine && "bg-amber-500/10",
                  isSelectedLine && "bg-amber-500/25",
                )}
              >
                <div className="select-none border-r px-1 text-right text-muted-foreground">
                  {previewLineNumber ?? ""}
                </div>
                <div className="min-w-0 whitespace-pre-wrap break-words px-2">
                  {content}
                </div>
              </div>
            )}
          </CellMeasurer>
        );
      }}
    />
  );
}

function SearchFileMatchList({
  fileMatches,
  isSearching,
  onSelectMatch,
  selectedPath,
}: SearchFileMatchListProps) {
  return (
    <div
      className="grid h-full min-h-0 w-full overflow-hidden text-xs"
      style={{
        gridTemplateRows: `${FILE_MATCH_HEADER_HEIGHT}px minmax(0, 1fr)`,
      }}
    >
      <div
        className="grid w-full border-b bg-muted text-[11px] font-medium text-muted-foreground"
        style={{ gridTemplateColumns: FILE_MATCH_GRID_COLUMNS }}
      >
        <div className="flex h-7 min-w-0 items-center px-2">File</div>
        <div className="flex h-7 min-w-0 items-center px-2">Lines</div>
        <div className="flex h-7 min-w-0 items-center px-2">Type</div>
        <div className="flex h-7 min-w-0 items-center px-2">Action</div>
      </div>

      <div className="min-h-0 w-full overflow-auto">
        {fileMatches.length > 0 ? (
          <div className="min-w-full">
            {fileMatches.map(({ match, matchedLines }) => {
              const isSelected = selectedPath === match.path;

              return (
                <div
                  key={match.path}
                  className={cn(
                    "grid cursor-default border-b text-xs",
                    isSelected && "bg-muted",
                  )}
                  style={{
                    gridTemplateColumns: FILE_MATCH_GRID_COLUMNS,
                    minHeight: FILE_MATCH_ROW_HEIGHT,
                  }}
                  onClick={() => onSelectMatch(match)}
                  onDoubleClick={() => onSelectMatch(match)}
                >
                  <div className="min-w-0 px-2 py-1">
                    <div className="flex min-w-0 items-start gap-1.5">
                      <FileCode2
                        className="mt-0.5 size-3.5 shrink-0 text-muted-foreground"
                        aria-hidden="true"
                      />
                      <div className="min-w-0">
                        <div className="truncate">{match.file}</div>
                        <div className="truncate font-mono text-[10px] text-muted-foreground">
                          {match.path}
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="flex min-w-0 items-center px-2 py-1">
                    {matchedLines.size.toLocaleString()}{" "}
                    {matchedLines.size === 1 ? "line" : "lines"}
                  </div>
                  <div className="flex min-w-0 items-center px-2 py-1">
                    <Badge
                      variant="outline"
                      className="h-5 max-w-full rounded-none px-1 text-[10px]"
                    >
                      <span className="truncate">{match.kind}</span>
                    </Badge>
                  </div>
                  <div className="flex min-w-0 items-center px-2 py-1">
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      className="h-6 rounded-none px-1.5 text-[11px]"
                      onClick={(event) => {
                        event.stopPropagation();
                        onSelectMatch(match);
                      }}
                    >
                      Open
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="flex h-24 items-center justify-center px-2 text-center text-xs text-muted-foreground">
            {isSearching ? "Searching..." : "Run search to show matches."}
          </div>
        )}
      </div>
    </div>
  );
}

function formatElapsedTime(milliseconds: number | null) {
  if (milliseconds === null) {
    return "-";
  }

  if (milliseconds < 1000) {
    return `${milliseconds} ms`;
  }

  return `${(milliseconds / 1000).toFixed(2)} s`;
}

function formatFileScanCount(
  scannedFiles: number | null,
  totalFiles: number | null,
  isSearching: boolean,
  totalComplete: boolean,
) {
  if (scannedFiles === null || totalFiles === null) {
    return isSearching ? "Starting..." : "-";
  }

  if (isSearching && !totalComplete) {
    return `${totalFiles.toLocaleString()} counted`;
  }

  if (isSearching && scannedFiles === totalFiles) {
    return `Scanning ${totalFiles.toLocaleString()}`;
  }

  return `${scannedFiles.toLocaleString()} / ${totalFiles.toLocaleString()}`;
}

export function SearchPage() {
  const { listing } = useEvidence();
  const { activeCase } = useCases();
  const [searchSettingsLoaded, setSearchSettingsLoaded] = useState(false);
  const [savedSearchSettings] = useState(loadSearchSettings);
  const [query, setQuery] = useState("");
  const [regex, setRegex] = useState(savedSearchSettings.regex);
  const [caseSensitive, setCaseSensitive] = useState(
    savedSearchSettings.caseSensitive,
  );
  const [binaryFiles, setBinaryFiles] = useState(
    savedSearchSettings.binaryFiles,
  );
  const [fileMatchVersion, setFileMatchVersion] = useState(0);
  const [matchCount, setMatchCount] = useState(0);
  const [matchedLineCount, setMatchedLineCount] = useState(0);
  const [selectedMatch, setSelectedMatch] = useState<SearchMatch | null>(null);
  const [activePreviewMatch, setActivePreviewMatch] =
    useState<SearchMatch | null>(null);
  const [currentPreviewLineMatches, setCurrentPreviewLineMatches] = useState<
    SearchMatch[]
  >([]);
  const [textPreview, setTextPreview] = useState<string[]>([]);
  const [hexPreview, setHexPreview] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [isSearching, setIsSearching] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [isPaneResizing, setIsPaneResizing] = useState(false);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);
  const [scannedFiles, setScannedFiles] = useState<number | null>(null);
  const [totalFiles, setTotalFiles] = useState<number | null>(null);
  const [isTotalFileCountComplete, setIsTotalFileCountComplete] =
    useState(false);
  const [liveElapsedMs, setLiveElapsedMs] = useState(0);
  const [wasCancelled, setWasCancelled] = useState(false);
  const [completedAt, setCompletedAt] = useState<Date | null>(null);
  const [activeSearchId, setActiveSearchId] = useState<string | null>(null);
  const [previewTab, setPreviewTab] = useState("text");
  const [dataSources, setDataSources] = useState<DataSourceRecord[]>([]);
  const [dataSourceError, setDataSourceError] = useState<string | null>(null);
  const [selectedScopeId, setSelectedScopeId] = useState("");
  const progressOffset = useRef({ scannedFiles: 0, totalFiles: 0 });
  const latestSearchId = useRef<string | null>(null);
  const cancelRequested = useRef(false);
  const firstStreamedMatch = useRef<SearchMatch | null>(null);
  const pendingStreamedSummaries = useRef<SearchFileSummary[]>([]);
  const fileMatchRows = useRef<FileMatch[]>([]);
  const fileMatchesByPath = useRef<Map<string, FileMatch>>(new Map());
  const lineMatchesByPath = useRef<Map<string, Map<number, SearchMatch>>>(
    new Map(),
  );
  const selectedMatchPath = useRef<string | null>(null);
  const matchCountRef = useRef(0);
  const matchedLineCountRef = useRef(0);
  const matchFlushTimer = useRef<number | null>(null);
  const paneResizeEndTimer = useRef<number | null>(null);
  const searchScopes = useMemo<SearchScope[]>(() => {
    const scopes: SearchScope[] = dataSources.map((dataSource) => ({
      id: `datasource:${dataSource.id}`,
      name: dataSource.name,
      description:
        dataSource.paths.length === 1
          ? dataSource.paths[0]
          : `${dataSource.paths.length} paths`,
      paths: dataSource.paths,
    }));

    if (listing) {
      scopes.push({
        id: `evidence:${listing.rootPath}`,
        name: listing.rootName,
        description: listing.rootPath,
        paths: [listing.rootPath],
      });
    }

    return scopes;
  }, [dataSources, listing]);
  const selectedScope =
    searchScopes.find((scope) => scope.id === selectedScopeId) ??
    searchScopes[0] ??
    null;
  const fileMatches = fileMatchRows.current;
  const selectedTextLineIndex = activePreviewMatch
    ? Math.min(
        Math.max(activePreviewMatch.line - 1, 0),
        Math.max(textPreview.length - 1, 0),
      )
    : 0;
  const reportFiles = useMemo(
    () =>
      fileMatches.map(({ match, matchCount }) => ({
        file: match.file,
        hits: matchCount,
        kind: match.kind,
        path: match.path,
      })),
    [fileMatchVersion],
  );
  const currentPreviewMatchLines = useMemo(
    () => new Set(currentPreviewLineMatches.map((match) => match.line)),
    [currentPreviewLineMatches],
  );
  const selectedPreviewLineIndex = activePreviewMatch
    ? currentPreviewLineMatches.findIndex(
        (match) => match.line === activePreviewMatch.line,
      )
    : -1;

  const canSearch = Boolean(selectedScope && query.trim() && !isSearching);
  const reportStatus: SearchReportStatus = isSearching
    ? "searching"
    : wasCancelled
      ? "cancelled"
      : elapsedMs === null
        ? "idle"
        : "completed";
  const reportRootPath = selectedScope
    ? selectedScope.paths.length === 1
      ? selectedScope.paths[0]
      : `${selectedScope.name} (${selectedScope.paths.length} paths)`
    : null;
  const reportText = useMemo(() => {
    if (previewTab !== "report") {
      return "";
    }

    return buildSearchReport({
      completedAt,
      elapsedMs: isSearching ? liveElapsedMs : elapsedMs,
      files: reportFiles,
      hitCount: matchCount,
      query,
      rootPath: reportRootPath,
      scannedFiles,
      status: reportStatus,
      totalFiles,
    });
  }, [
    completedAt,
    elapsedMs,
    isSearching,
    liveElapsedMs,
    matchCount,
    previewTab,
    query,
    reportFiles,
    reportRootPath,
    reportStatus,
    scannedFiles,
    totalFiles,
  ]);
  const commandPreview = selectedScope
    ? `grep crate ${regex ? "regex" : "fixed"} ${
        caseSensitive ? "case-sensitive" : "ignore-case"
      } ${binaryFiles ? "binary-as-text" : "skip-binary"} "${
        query || "<query>"
      }" ${selectedScope.paths.join(", ")}`
    : "Add a datasource or open an evidence directory before searching";

  function cancelPendingMatchFlush() {
    if (matchFlushTimer.current !== null) {
      window.clearTimeout(matchFlushTimer.current);
      matchFlushTimer.current = null;
    }

    pendingStreamedSummaries.current = [];
  }

  function endPaneResizeSoon() {
    if (paneResizeEndTimer.current !== null) {
      window.clearTimeout(paneResizeEndTimer.current);
    }

    paneResizeEndTimer.current = window.setTimeout(() => {
      paneResizeEndTimer.current = null;
      setIsPaneResizing(false);
    }, 80);
  }

  function beginPaneResize() {
    if (paneResizeEndTimer.current !== null) {
      window.clearTimeout(paneResizeEndTimer.current);
      paneResizeEndTimer.current = null;
    }

    setIsPaneResizing(true);

    const finishResize = () => {
      window.removeEventListener("pointerup", finishResize);
      window.removeEventListener("pointercancel", finishResize);
      window.removeEventListener("blur", finishResize);
      endPaneResizeSoon();
    };

    window.addEventListener("pointerup", finishResize);
    window.addEventListener("pointercancel", finishResize);
    window.addEventListener("blur", finishResize);
  }

  function getPreviewLineMatches(path: string | null) {
    if (!path) {
      return [];
    }

    return Array.from(lineMatchesByPath.current.get(path)?.values() ?? []).sort(
      (first, second) => first.line - second.line || first.column - second.column,
    );
  }

  function resetStreamedMatches() {
    cancelPendingMatchFlush();
    firstStreamedMatch.current = null;
    fileMatchRows.current = [];
    fileMatchesByPath.current.clear();
    lineMatchesByPath.current.clear();
    selectedMatchPath.current = null;
    matchCountRef.current = 0;
    matchedLineCountRef.current = 0;
    setFileMatchVersion((version) => version + 1);
    setMatchCount(0);
    setMatchedLineCount(0);
    setCurrentPreviewLineMatches([]);
  }

  function flushStreamedMatches(searchId: string) {
    if (matchFlushTimer.current !== null) {
      window.clearTimeout(matchFlushTimer.current);
      matchFlushTimer.current = null;
    }

    if (latestSearchId.current !== searchId) {
      pendingStreamedSummaries.current = [];
      return;
    }

    const nextSummaries = pendingStreamedSummaries.current;

    if (nextSummaries.length === 0) {
      return;
    }

    pendingStreamedSummaries.current = [];
    const shouldSelectFirstMatch = firstStreamedMatch.current === null;
    const firstMatch = nextSummaries[0]?.firstMatch ?? null;
    const selectedPath = selectedMatchPath.current;
    let didUpdateSelectedPath = false;

    if (shouldSelectFirstMatch && firstMatch) {
      firstStreamedMatch.current = firstMatch;
    }

    for (const summary of nextSummaries) {
      matchCountRef.current += summary.matchCount;
      let lineMatches = lineMatchesByPath.current.get(summary.path);

      if (!lineMatches) {
        lineMatches = new Map();
        lineMatchesByPath.current.set(summary.path, lineMatches);
      }

      for (const line of summary.matchedLines) {
        const isNewLine = !lineMatches.has(line);

        if (isNewLine) {
          lineMatches.set(line, {
            ...summary.firstMatch,
            id: `${summary.path}:${line}:summary`,
            line,
          });
          matchedLineCountRef.current += 1;
        }
      }

      let fileMatch = fileMatchesByPath.current.get(summary.path);

      if (fileMatch) {
        fileMatch.matchCount += summary.matchCount;

        for (const line of summary.matchedLines) {
          fileMatch.matchedLines.add(line);
        }

        if (
          summary.firstMatch.line < fileMatch.match.line ||
          (summary.firstMatch.line === fileMatch.match.line &&
            summary.firstMatch.column < fileMatch.match.column)
        ) {
          fileMatch.match = summary.firstMatch;
        }
      } else {
        fileMatch = {
          match: summary.firstMatch,
          matchCount: summary.matchCount,
          matchedLines: new Set(summary.matchedLines),
        };
        fileMatchesByPath.current.set(summary.path, fileMatch);
        fileMatchRows.current.push(fileMatch);
      }

      didUpdateSelectedPath =
        didUpdateSelectedPath || selectedPath === summary.path;
    }

    setMatchCount(matchCountRef.current);
    setMatchedLineCount(matchedLineCountRef.current);
    setFileMatchVersion((version) => version + 1);

    if (shouldSelectFirstMatch && firstMatch) {
      selectedMatchPath.current = firstMatch.path;
      setSelectedMatch((currentMatch) => currentMatch ?? firstMatch);
      setActivePreviewMatch((currentMatch) => currentMatch ?? firstMatch);
      setCurrentPreviewLineMatches(getPreviewLineMatches(firstMatch.path));
    } else if (didUpdateSelectedPath) {
      setCurrentPreviewLineMatches(getPreviewLineMatches(selectedPath));
    }
  }

  function queueStreamedSummaries(
    searchId: string,
    nextSummaries: SearchFileSummary[],
  ) {
    if (nextSummaries.length === 0 || latestSearchId.current !== searchId) {
      return;
    }

    pendingStreamedSummaries.current.push(...nextSummaries);

    if (matchFlushTimer.current !== null) {
      return;
    }

    const flushDelay =
      matchCountRef.current === 0 ? 0 : SEARCH_UI_MATCH_FLUSH_INTERVAL_MS;

    matchFlushTimer.current = window.setTimeout(() => {
      matchFlushTimer.current = null;
      flushStreamedMatches(searchId);
    }, flushDelay);
  }

  async function loadFinishedMatchDetails(path: string) {
    const detailsQuery = query;
    const detailsRegex = regex;
    const detailsCaseSensitive = caseSensitive;
    const detailsBinaryFiles = binaryFiles;

    if (!detailsQuery.trim()) {
      return;
    }

    try {
      const detailedMatches = await invoke<SearchMatch[]>(
        "read_search_match_details",
        {
          path,
          query: detailsQuery,
          regex: detailsRegex,
          caseSensitive: detailsCaseSensitive,
          binaryFiles: detailsBinaryFiles,
        },
      );

      if (selectedMatchPath.current !== path || detailedMatches.length === 0) {
        return;
      }

      const lineMatches = new Map<number, SearchMatch>();

      for (const match of detailedMatches) {
        if (!lineMatches.has(match.line)) {
          lineMatches.set(match.line, match);
        }
      }

      lineMatchesByPath.current.set(path, lineMatches);
      setCurrentPreviewLineMatches(getPreviewLineMatches(path));
    } catch (caughtError) {
      setError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
    }
  }

  async function runSearch() {
    if (!selectedScope || !query.trim()) {
      return;
    }

    const searchId =
      typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.random()}`;

    setIsSearching(true);
    setActiveSearchId(searchId);
    latestSearchId.current = searchId;
    cancelRequested.current = false;
    resetStreamedMatches();
    setError(null);
    setElapsedMs(null);
    setScannedFiles(null);
    setTotalFiles(null);
    setIsTotalFileCountComplete(false);
    setLiveElapsedMs(0);
    setWasCancelled(false);
    setCompletedAt(null);
    setSelectedMatch(null);
    setActivePreviewMatch(null);
    progressOffset.current = { scannedFiles: 0, totalFiles: 0 };
    const searchStartedAt = performance.now();

    try {
      let nextScannedFiles = 0;
      let nextTotalFiles = 0;
      let nextTotalComplete = true;
      let nextWasCancelled = false;

      for (const rootPath of selectedScope.paths) {
        if (cancelRequested.current) {
          nextWasCancelled = true;
          break;
        }

        progressOffset.current = {
          scannedFiles: nextScannedFiles,
          totalFiles: nextTotalFiles,
        };

        const result = await invoke<SearchResult>("search_files", {
          request: {
            searchId,
            rootPath,
            query,
            regex,
            caseSensitive,
            binaryFiles,
          },
        });

        if (latestSearchId.current !== searchId) {
          return;
        }

        flushStreamedMatches(searchId);
        nextScannedFiles += result.scannedFiles;
        nextTotalFiles += result.totalFiles;
        nextTotalComplete = nextTotalComplete && result.totalComplete;
        nextWasCancelled =
          nextWasCancelled || result.cancelled || cancelRequested.current;

        setScannedFiles(nextScannedFiles);
        setTotalFiles(nextTotalFiles);
        setIsTotalFileCountComplete(nextTotalComplete);

        if (result.cancelled || cancelRequested.current) {
          break;
        }
      }

      flushStreamedMatches(searchId);
      const firstSearchMatch = firstStreamedMatch.current;

      if (!selectedMatchPath.current && firstSearchMatch) {
        selectedMatchPath.current = firstSearchMatch.path;
        setCurrentPreviewLineMatches(
          getPreviewLineMatches(firstSearchMatch.path),
        );
      }

      setSelectedMatch((currentMatch) => currentMatch ?? firstSearchMatch);
      setActivePreviewMatch(
        (currentMatch) => currentMatch ?? firstSearchMatch,
      );

      if (firstSearchMatch) {
        void loadFinishedMatchDetails(firstSearchMatch.path);
      }

      setElapsedMs(Math.round(performance.now() - searchStartedAt));
      setScannedFiles(nextScannedFiles);
      setTotalFiles(nextTotalFiles);
      setIsTotalFileCountComplete(nextTotalComplete);
      setWasCancelled(nextWasCancelled);
      setCompletedAt(new Date());
    } catch (caughtError) {
      setError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
      resetStreamedMatches();
      setSelectedMatch(null);
      setActivePreviewMatch(null);
      setScannedFiles(null);
      setTotalFiles(null);
      setIsTotalFileCountComplete(false);
      setElapsedMs(Math.round(performance.now() - searchStartedAt));
      setCompletedAt(new Date());
    } finally {
      if (latestSearchId.current === searchId) {
        setIsSearching(false);
        setIsCancelling(false);
        setActiveSearchId(null);
        cancelRequested.current = false;
      }
    }
  }

  async function cancelActiveSearch() {
    if (!activeSearchId || isCancelling) {
      return;
    }

    const searchId = activeSearchId;
    cancelRequested.current = true;
    setIsCancelling(true);
    setError(null);

    try {
      const didCancel = await invoke<boolean>("cancel_search", { searchId });

      if (!didCancel) {
        cancelRequested.current = false;
        setIsCancelling(false);
      }
    } catch (caughtError) {
      cancelRequested.current = false;
      setError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
      setIsCancelling(false);
    }
  }

  function selectMatch(match: SearchMatch) {
    selectedMatchPath.current = match.path;
    setSelectedMatch(match);
    setActivePreviewMatch(match);
    setCurrentPreviewLineMatches(getPreviewLineMatches(match.path));
    void loadFinishedMatchDetails(match.path);
  }

  function selectRelativePreviewLine(direction: -1 | 1) {
    if (currentPreviewLineMatches.length === 0) {
      return;
    }

    const currentIndex =
      selectedPreviewLineIndex === -1 ? 0 : selectedPreviewLineIndex;
    const nextIndex =
      (currentIndex + direction + currentPreviewLineMatches.length) %
      currentPreviewLineMatches.length;

    setActivePreviewMatch(currentPreviewLineMatches[nextIndex]);
  }

  useEffect(() => {
    let isDisposed = false;
    const unlistenCallbacks: Array<() => void> = [];

    void listen<SearchProgress>("search-progress", (event) => {
      if (event.payload.searchId !== latestSearchId.current) {
        return;
      }

      setScannedFiles(
        progressOffset.current.scannedFiles + event.payload.scannedFiles,
      );
      setTotalFiles(
        progressOffset.current.totalFiles + event.payload.totalFiles,
      );
      setIsTotalFileCountComplete(event.payload.totalComplete);
    }).then((nextUnlisten) => {
      if (isDisposed) {
        nextUnlisten();
        return;
      }

      unlistenCallbacks.push(nextUnlisten);
    });

    void listen<SearchSummaries>("search-summaries", (event) => {
      if (event.payload.searchId !== latestSearchId.current) {
        return;
      }

      queueStreamedSummaries(event.payload.searchId, event.payload.files);
    }).then((nextUnlisten) => {
      if (isDisposed) {
        nextUnlisten();
        return;
      }

      unlistenCallbacks.push(nextUnlisten);
    });

    return () => {
      isDisposed = true;
      cancelPendingMatchFlush();
      unlistenCallbacks.forEach((unlisten) => {
        unlisten();
      });
    };
  }, []);

  useEffect(() => {
    if (!activeCase) {
      setDataSources([]);
      setDataSourceError(null);
      return;
    }

    let isCurrent = true;

    listDataSources(activeCase.databasePath, activeCase.id)
      .then((nextDataSources) => {
        if (isCurrent) {
          setDataSources(nextDataSources);
          setDataSourceError(null);
        }
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setDataSourceError(
          caughtError instanceof Error
            ? caughtError.message
            : String(caughtError),
        );
        setDataSources([]);
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase]);

  useEffect(() => {
    if (!activeCase) {
      return;
    }

    return subscribeToDataSourcesChanged((caseId) => {
      if (caseId !== activeCase.id) {
        return;
      }

      listDataSources(activeCase.databasePath, activeCase.id)
        .then((nextDataSources) => {
          setDataSources(nextDataSources);
          setDataSourceError(null);
        })
        .catch((caughtError) => {
          setDataSourceError(
            caughtError instanceof Error
              ? caughtError.message
              : String(caughtError),
          );
        });
    });
  }, [activeCase]);

  useEffect(() => {
    if (searchScopes.length === 0) {
      setSelectedScopeId("");
      return;
    }

    if (!searchScopes.some((scope) => scope.id === selectedScopeId)) {
      setSelectedScopeId(searchScopes[0].id);
    }
  }, [searchScopes, selectedScopeId]);

  useEffect(() => {
    if (!searchSettingsLoaded) {
      setSearchSettingsLoaded(true);
      return;
    }

    saveSearchSettings({
      binaryFiles,
      caseSensitive,
      regex,
    });
  }, [binaryFiles, caseSensitive, regex, searchSettingsLoaded]);

  useEffect(() => {
    if (!isSearching) {
      return;
    }

    const searchStartedAt = performance.now();
    const timer = window.setInterval(() => {
      setLiveElapsedMs(Math.round(performance.now() - searchStartedAt));
    }, 100);

    return () => window.clearInterval(timer);
  }, [isSearching]);

  useEffect(() => {
    return () => {
      if (paneResizeEndTimer.current !== null) {
        window.clearTimeout(paneResizeEndTimer.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!selectedMatch) {
      setTextPreview([]);
      setHexPreview([]);
      setActivePreviewMatch(null);
      return;
    }

    let isCurrent = true;
    setIsPreviewLoading(true);

    Promise.all([
      invoke<string[]>("read_text_preview", {
        path: selectedMatch.path,
        line: selectedMatch.line,
      }),
      invoke<string[]>("read_hex_preview", { path: selectedMatch.path }),
    ])
      .then(([nextTextPreview, nextHexPreview]) => {
        if (!isCurrent) {
          return;
        }

        setTextPreview(nextTextPreview);
        setHexPreview(nextHexPreview);
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setError(
          caughtError instanceof Error
            ? caughtError.message
            : String(caughtError),
        );
        setTextPreview([]);
        setHexPreview([]);
      })
      .finally(() => {
        if (isCurrent) {
          setIsPreviewLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [selectedMatch?.path]);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <div className="relative w-80">
          <Search
            className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            className="h-7 rounded-none pl-7 text-xs"
            value={query}
            placeholder="Search logical files..."
            aria-label="Search query"
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                void runSearch();
              }
            }}
          />
        </div>
        <Button
          size="xs"
          className="h-7 rounded-none px-2 text-xs"
          disabled={isSearching ? isCancelling : !canSearch}
          onClick={() => {
            if (isSearching) {
              void cancelActiveSearch();
            } else {
              void runSearch();
            }
          }}
        >
          {isSearching ? (
            <Square className="size-3.5" aria-hidden="true" />
          ) : (
            <Play className="size-3.5" aria-hidden="true" />
          )}
          {isSearching ? (isCancelling ? "Cancelling" : "Cancel") : "Run"}
        </Button>
        <Separator orientation="vertical" className="h-5" />
        <Button
          variant={regex ? "secondary" : "ghost"}
          size="xs"
          className="h-7 rounded-none px-2 text-xs"
          onClick={() => setRegex((value) => !value)}
        >
          <Regex className="size-3.5" aria-hidden="true" />
          Regex
        </Button>
        <Button
          variant={caseSensitive ? "secondary" : "ghost"}
          size="xs"
          className="h-7 rounded-none px-2 text-xs"
          onClick={() => setCaseSensitive((value) => !value)}
        >
          <CaseSensitive className="size-3.5" aria-hidden="true" />
          Case
        </Button>
        <Dialog>
          <DialogTrigger asChild>
            <Button
              variant={binaryFiles ? "secondary" : "ghost"}
              size="xs"
              className="h-7 rounded-none px-2 text-xs"
            >
              <SlidersHorizontal className="size-3.5" aria-hidden="true" />
              Options
              {binaryFiles && (
                <Badge
                  variant="outline"
                  className="ml-0.5 h-4 rounded-none px-1 text-[10px]"
                >
                  Binary
                </Badge>
              )}
            </Button>
          </DialogTrigger>
          <DialogContent className="max-w-md rounded-none p-0">
            <DialogHeader className="border-b px-3 py-2">
              <DialogTitle className="text-sm">Search Options</DialogTitle>
              <DialogDescription className="text-xs">
                Configure search flags for the next run.
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-2 px-3 py-3">
              <label className="flex cursor-pointer items-start gap-2 text-xs">
                <Checkbox
                  className="mt-0.5 rounded-none"
                  checked={binaryFiles}
                  onCheckedChange={(checked) =>
                    setBinaryFiles(checked === true)
                  }
                />
                <span className="grid gap-0.5">
                  <span className="font-medium">Search binary files</span>
                  <span className="text-[11px] text-muted-foreground">
                    Searches binary files as text instead of stopping at NUL
                    bytes.
                  </span>
                </span>
              </label>
            </div>
            <DialogFooter showCloseButton className="border-t px-3 py-2" />
          </DialogContent>
        </Dialog>
        <div className="ml-auto flex items-center gap-2 text-[11px] text-muted-foreground">
          <span>{selectedScope ? selectedScope.name : "No search scope"}</span>
          <span>
            Time:{" "}
            {isSearching
              ? formatElapsedTime(liveElapsedMs)
              : formatElapsedTime(elapsedMs)}
          </span>
          <span>
            Files:{" "}
            {formatFileScanCount(
              scannedFiles,
              totalFiles,
              isSearching,
              isTotalFileCountComplete,
            )}
          </span>
          <Badge variant="outline" className="h-5 rounded-none text-[11px]">
            {fileMatches.length} files / {matchedLineCount} lines
          </Badge>
        </div>
      </section>

      {(error || dataSourceError) && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{error ?? dataSourceError}</span>
        </section>
      )}

      <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2">
        <FolderOpen
          className="size-3.5 text-muted-foreground"
          aria-hidden="true"
        />
        <select
          className="h-6 max-w-72 rounded-none border bg-background px-2 text-xs"
          value={selectedScope?.id ?? ""}
          title={selectedScope?.description}
          aria-label="Search scope"
          disabled={isSearching || searchScopes.length === 0}
          onChange={(event) => setSelectedScopeId(event.target.value)}
        >
          {searchScopes.map((scope) => (
            <option key={scope.id} value={scope.id} title={scope.description}>
              {scope.name}
            </option>
          ))}
        </select>
        <Separator orientation="vertical" className="h-5" />
        <span className="truncate text-xs">{commandPreview}</span>
      </section>

      <ResizablePanelGroup
        orientation="horizontal"
        className="min-h-0 min-w-0 flex-1"
        onLayoutChanged={endPaneResizeSoon}
      >
        <ResizablePanel defaultSize="36%" minSize="22%">
          <section
            className="h-full min-h-0 min-w-0 overflow-hidden border-r"
            aria-label="Search matches"
          >
            {isPaneResizing ? (
              <div className="flex h-full items-center justify-center px-2 text-xs text-muted-foreground">
                {fileMatches.length.toLocaleString()} matched files
              </div>
            ) : (
              <SearchFileMatchList
                fileMatches={fileMatches}
                isSearching={isSearching}
                onSelectMatch={selectMatch}
                selectedPath={selectedMatch?.path ?? null}
              />
            )}
          </section>
        </ResizablePanel>

        <ResizableHandle withHandle onPointerDown={beginPaneResize} />

        <ResizablePanel defaultSize="64%" minSize="34%">
          <section
            className="h-full min-h-0 min-w-0 overflow-hidden"
            aria-label="File preview"
          >
            {isPaneResizing ? (
              <div className="flex h-full items-center justify-center px-2 text-xs text-muted-foreground">
                Preview paused while resizing
              </div>
            ) : (
              <Tabs
                value={previewTab}
                onValueChange={setPreviewTab}
                className="flex h-full min-h-0 min-w-0 flex-col gap-0"
              >
            <div className="flex h-8 items-center justify-between border-b px-2">
              <div className="flex min-w-0 items-center gap-2 text-xs">
                <span className="font-medium">Preview: </span>
                <span className="truncate text-muted-foreground">
                  {selectedMatch?.path ?? "No match selected"}
                </span>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  className="h-7 rounded-none px-2 text-xs"
                  disabled={currentPreviewLineMatches.length <= 1}
                  onClick={() => selectRelativePreviewLine(-1)}
                >
                  <ChevronLeft className="size-3.5" aria-hidden="true" />
                  Back
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  className="h-7 rounded-none px-2 text-xs"
                  disabled={currentPreviewLineMatches.length <= 1}
                  onClick={() => selectRelativePreviewLine(1)}
                >
                  Next
                  <ChevronRight className="size-3.5" aria-hidden="true" />
                </Button>
                <Separator orientation="vertical" className="h-5" />
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
                    value="hex"
                    className="h-7 rounded-none px-2 text-xs"
                  >
                    <Hexagon className="size-3.5" aria-hidden="true" />
                    Hex
                  </TabsTrigger>
                  <TabsTrigger
                    value="report"
                    className="h-7 rounded-none px-2 text-xs"
                  >
                    <FileText className="size-3.5" aria-hidden="true" />
                    Report
                  </TabsTrigger>
                </TabsList>
              </div>
            </div>

            <TabsContent
              value="text"
              className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
            >
              <div className="h-full min-h-0 min-w-0 overflow-hidden p-2">
                {textPreview.length > 0 ? (
                  <AutoSizer>
                    {({ height, width }) => (
                      <TextPreviewList
                        activePreviewMatch={activePreviewMatch}
                        currentPreviewMatchLines={currentPreviewMatchLines}
                        width={width}
                        height={height}
                        lines={textPreview}
                        selectedTextLineIndex={selectedTextLineIndex}
                      />
                    )}
                  </AutoSizer>
                ) : (
                  <div className="font-mono text-xs text-muted-foreground">
                    {isPreviewLoading
                      ? "Loading preview..."
                      : "Select a match to preview text."}
                  </div>
                )}
              </div>
            </TabsContent>

            <TabsContent
              value="hex"
              className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
            >
              <ScrollArea className="h-full min-h-0">
                <pre className="p-2 font-mono text-xs leading-5">
                  {hexPreview.map((line) => (
                    <div key={line} className="whitespace-pre-wrap">
                      {line}
                    </div>
                  ))}
                  {hexPreview.length === 0 && (
                    <div className="text-muted-foreground">
                      {isPreviewLoading
                        ? "Loading preview..."
                        : "Select a match to preview hex."}
                    </div>
                  )}
                </pre>
              </ScrollArea>
            </TabsContent>

            <TabsContent
              value="report"
              className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
            >
              <ScrollArea className="h-full min-h-0">
                <pre className="p-2 font-mono text-xs leading-5">
                  {reportText}
                </pre>
              </ScrollArea>
            </TabsContent>
          </Tabs>
            )}
        </section>
        </ResizablePanel>
      </ResizablePanelGroup>

      <footer className="flex h-6 shrink-0 items-center gap-3 border-t px-2 text-[11px] text-muted-foreground">
        <span>{isSearching ? "Searching" : "Search idle"}</span>
        <span>Engine: grep crate</span>
        <span>Scope: {listing?.rootName ?? "none"}</span>
        <span>Time: {formatElapsedTime(isSearching ? liveElapsedMs : elapsedMs)}</span>
        <span>
          Files:{" "}
          {formatFileScanCount(
            scannedFiles,
            totalFiles,
            isSearching,
            isTotalFileCountComplete,
          )}
        </span>
        {wasCancelled && <span>Cancelled</span>}
        <span>Preview: text/hex</span>
      </footer>
    </div>
  );
}
