import {
  AutoSizer,
  CellMeasurer,
  CellMeasurerCache,
  List,
  type ListRowProps,
} from "react-virtualized";
import "react-virtualized/styles.css";
import { FileCode2, Hexagon } from "lucide-react";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";

type FilePreviewViewerProps = {
  activeTab: FilePreviewTab;
  hexPreview: string[];
  isLoading: boolean;
  onActiveTabChange: (tab: FilePreviewTab) => void;
  selectedEntry: EvidenceDirectoryEntry | null;
  textPreview: string[];
};

export type FilePreviewTab = "text" | "hex";

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

export function FilePreviewViewer({
  activeTab,
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
