import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AutoSizer,
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useEvidence } from "@/features/evidence/evidence-provider";
import { cn } from "@/lib/utils";

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
};

function formatElapsedTime(milliseconds: number | null) {
  if (milliseconds === null) {
    return "-";
  }

  if (milliseconds < 1000) {
    return `${milliseconds} ms`;
  }

  return `${(milliseconds / 1000).toFixed(2)} s`;
}

export function SearchPage() {
  const { listing } = useEvidence();
  const [query, setQuery] = useState("");
  const [regex, setRegex] = useState(false);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [binaryFiles, setBinaryFiles] = useState(false);
  const [matches, setMatches] = useState<SearchMatch[]>([]);
  const [selectedMatch, setSelectedMatch] = useState<SearchMatch | null>(null);
  const [activePreviewMatch, setActivePreviewMatch] =
    useState<SearchMatch | null>(null);
  const [textPreview, setTextPreview] = useState<string[]>([]);
  const [hexPreview, setHexPreview] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [isSearching, setIsSearching] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);
  const [liveElapsedMs, setLiveElapsedMs] = useState(0);
  const [wasCancelled, setWasCancelled] = useState(false);
  const [activeSearchId, setActiveSearchId] = useState<string | null>(null);
  const textPreviewList = useRef<List | null>(null);
  const textPreviewLineHeight = 20;
  const selectedTextLineIndex = activePreviewMatch
    ? Math.max(activePreviewMatch.line - 1, 0)
    : 0;
  const currentPreviewLineMatches = selectedMatch
    ? Array.from(
        matches
          .filter((match) => match.path === selectedMatch.path)
          .sort((first, second) => first.line - second.line || first.column - second.column)
          .reduce((lineMatches, match) => {
            if (!lineMatches.has(match.line)) {
              lineMatches.set(match.line, match);
            }

            return lineMatches;
          }, new Map<number, SearchMatch>())
          .values(),
      )
    : [];
  const currentPreviewMatchLines = new Set(
    currentPreviewLineMatches.map((match) => match.line),
  );
  const selectedPreviewLineIndex = activePreviewMatch
    ? currentPreviewLineMatches.findIndex(
        (match) => match.line === activePreviewMatch.line,
      )
    : -1;

  const canSearch = Boolean(listing && query.trim() && !isSearching);
  const commandPreview = listing
    ? `rg --json --line-number --column ${
        regex ? "" : "--fixed-strings "
      }${caseSensitive ? "" : "--ignore-case "}${
        binaryFiles ? "--binary " : ""
      }"${query || "<query>"}" ${
        listing.rootPath
      }`
    : "Open an evidence directory before searching";

  async function runSearch() {
    if (!listing || !query.trim()) {
      return;
    }

    const searchId =
      typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.random()}`;

    setIsSearching(true);
    setActiveSearchId(searchId);
    setError(null);
    setElapsedMs(null);
    setLiveElapsedMs(0);
    setWasCancelled(false);
    const searchStartedAt = performance.now();

    try {
      const result = await invoke<SearchResult>("search_files", {
        searchId,
        rootPath: listing.rootPath,
        query,
        regex,
        caseSensitive,
        binaryFiles,
      });

      setMatches(result.matches);
      setSelectedMatch(result.matches[0] ?? null);
      setActivePreviewMatch(result.matches[0] ?? null);
      setElapsedMs(result.elapsedMs);
      setWasCancelled(result.cancelled);
    } catch (caughtError) {
      setError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
      setMatches([]);
      setSelectedMatch(null);
      setActivePreviewMatch(null);
      setElapsedMs(Math.round(performance.now() - searchStartedAt));
    } finally {
      setIsSearching(false);
      setIsCancelling(false);
      setActiveSearchId(null);
    }
  }

  async function cancelActiveSearch() {
    if (!activeSearchId || isCancelling) {
      return;
    }

    setIsCancelling(true);
    setError(null);

    try {
      await invoke<boolean>("cancel_search", { searchId: activeSearchId });
    } catch (caughtError) {
      setError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
      setIsCancelling(false);
    }
  }

  function selectMatch(match: SearchMatch) {
    setSelectedMatch(match);
    setActivePreviewMatch(match);
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

  useEffect(() => {
    if (!activePreviewMatch || textPreview.length === 0) {
      return;
    }

    textPreviewList.current?.scrollToRow(selectedTextLineIndex);
  }, [activePreviewMatch, selectedTextLineIndex, textPreview.length]);

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
          {isSearching ? (isCancelling ? "Cancelling" : "Cancel") : "Run rg"}
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
              <DialogTitle className="text-sm">Ripgrep Options</DialogTitle>
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
                    Adds <span className="font-mono">--binary</span> so
                    ripgrep does not skip files detected as binary.
                  </span>
                </span>
              </label>
            </div>
            <DialogFooter showCloseButton className="border-t px-3 py-2" />
          </DialogContent>
        </Dialog>
        <div className="ml-auto flex items-center gap-2 text-[11px] text-muted-foreground">
          <span>{listing ? listing.rootName : "No evidence mounted"}</span>
          <span>
            Time:{" "}
            {isSearching
              ? formatElapsedTime(liveElapsedMs)
              : formatElapsedTime(elapsedMs)}
          </span>
          <Badge variant="outline" className="h-5 rounded-none text-[11px]">
            {matches.length} matches
          </Badge>
        </div>
      </section>

      {error && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{error}</span>
        </section>
      )}

      <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2">
        <FolderOpen
          className="size-3.5 text-muted-foreground"
          aria-hidden="true"
        />
        <span className="truncate text-xs">{commandPreview}</span>
      </section>

      <ResizablePanelGroup
        orientation="vertical"
        className="min-h-0 min-w-0 flex-1"
      >
        <ResizablePanel defaultSize="58%" minSize="28%">
          <section
            className="h-full min-h-0 min-w-0 overflow-hidden border-b"
            aria-label="Search matches"
          >
            <div className="h-full min-w-0 overflow-auto text-xs" tabIndex={0}>
              <Table
                containerClassName="contents"
                className="w-max min-w-full table-auto caption-bottom text-xs"
              >
                <TableHeader className="sticky top-0 z-10 bg-muted">
                  <TableRow className="h-7">
                    <TableHead className="h-7 min-w-40 px-2 text-[11px]">
                      File
                    </TableHead>
                    <TableHead className="h-7 min-w-80 px-2 text-[11px]">
                      Path
                    </TableHead>
                    <TableHead className="h-7 min-w-24 px-2 text-[11px]">
                      Line
                    </TableHead>
                    <TableHead className="h-7 min-w-24 px-2 text-[11px]">
                      Type
                    </TableHead>
                    <TableHead className="h-7 min-w-96 px-2 text-[11px]">
                      Match
                    </TableHead>
                    <TableHead className="h-7 min-w-20 px-2 text-[11px]">
                      Action
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {matches.map((match) => (
                    <TableRow
                      key={match.id}
                      data-state={
                        selectedMatch?.id === match.id ? "selected" : undefined
                      }
                      className="h-8 cursor-default"
                      onClick={() => selectMatch(match)}
                      onDoubleClick={() => selectMatch(match)}
                    >
                      <TableCell className="px-2 py-1">
                        <div className="flex items-center gap-1.5">
                          <FileCode2
                            className="size-3.5 shrink-0 text-muted-foreground"
                            aria-hidden="true"
                          />
                          <span>{match.file}</span>
                        </div>
                      </TableCell>
                      <TableCell className="px-2 py-1 font-mono text-[11px]">
                        {match.path}
                      </TableCell>
                      <TableCell className="px-2 py-1">
                        {match.line}:{match.column}
                      </TableCell>
                      <TableCell className="px-2 py-1">
                        <Badge
                          variant="outline"
                          className="h-5 rounded-none px-1 text-[10px]"
                        >
                          {match.kind}
                        </Badge>
                      </TableCell>
                      <TableCell className="px-2 py-1">
                        <div className="flex flex-col">
                          <span>{match.matchedText}</span>
                          <span className="text-[11px] text-muted-foreground">
                            {match.context}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell className="px-2 py-1">
                        <Button
                          type="button"
                          variant="ghost"
                          size="xs"
                          className="h-6 rounded-none px-1.5 text-[11px]"
                          onClick={(event) => {
                            event.stopPropagation();
                            selectMatch(match);
                          }}
                        >
                          Open
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))}
                  {matches.length === 0 && (
                    <TableRow>
                      <TableCell
                        colSpan={6}
                        className="h-24 px-2 text-center text-xs text-muted-foreground"
                      >
                        {isSearching
                          ? "Searching..."
                          : "Run ripgrep to show matches."}
                      </TableCell>
                    </TableRow>
                  )}
                </TableBody>
              </Table>
            </div>
          </section>
        </ResizablePanel>

        <ResizableHandle withHandle />

        <ResizablePanel defaultSize="42%" minSize="18%">
          <section
            className="h-full min-h-0 min-w-0 overflow-hidden"
            aria-label="File preview"
          >
            <Tabs
              defaultValue="text"
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
                      <List
                        ref={(list) => {
                          textPreviewList.current = list;
                        }}
                        className="font-mono text-xs"
                        width={width}
                        height={height}
                        rowCount={textPreview.length}
                        rowHeight={textPreviewLineHeight}
                        overscanRowCount={12}
                        scrollToAlignment="center"
                        scrollToIndex={selectedTextLineIndex}
                        rowRenderer={({ index, key, style }: ListRowProps) => {
                          const line = textPreview[index];
                          const previewLineNumber = Number.parseInt(
                            line.trimStart(),
                            10,
                          );
                          const isSelectedLine =
                            activePreviewMatch?.line === previewLineNumber;
                          const isMatchedLine =
                            currentPreviewMatchLines.has(previewLineNumber);

                          return (
                            <div
                              key={key}
                              style={style}
                              className={cn(
                                "whitespace-pre px-1 leading-5",
                                isMatchedLine && "bg-amber-500/10",
                                isSelectedLine && "bg-amber-500/25",
                              )}
                            >
                              {line}
                            </div>
                          );
                        }}
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
          </Tabs>
        </section>
        </ResizablePanel>
      </ResizablePanelGroup>

      <footer className="flex h-6 shrink-0 items-center gap-3 border-t px-2 text-[11px] text-muted-foreground">
        <span>{isSearching ? "Searching" : "Search idle"}</span>
        <span>Engine: ripgrep</span>
        <span>Scope: {listing?.rootName ?? "none"}</span>
        <span>Time: {formatElapsedTime(isSearching ? liveElapsedMs : elapsedMs)}</span>
        {wasCancelled && <span>Cancelled</span>}
        <span>Preview: text/hex</span>
      </footer>
    </div>
  );
}
