import {
  AutoSizer,
  CellMeasurer,
  CellMeasurerCache,
  List,
  type ListRowProps,
} from "react-virtualized";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";

type TextPreviewViewerProps = {
  emptyText: string;
  isLoading: boolean;
  lines: string[];
};

type PreviewLinesListProps = TextPreviewViewerProps & {
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
  // Rust formats text preview rows as "<line number><two spaces><content>".
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
  // Wrapped rows can grow taller, so react-virtualized measures each row after render.
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
  // Fixed-height rows make unwrapped mode cheap to virtualize while keeping horizontal scroll.
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

export function TextPreviewViewer({
  emptyText,
  isLoading,
  lines,
}: TextPreviewViewerProps) {
  // Word wrap is a viewer preference, not file state, so persist it locally across selections.
  const [isWordWrapEnabled, setIsWordWrapEnabled] = useState(
    loadTextWordWrapSetting,
  );

  useEffect(() => {
    saveTextWordWrapSetting(isWordWrapEnabled);
  }, [isWordWrapEnabled]);

  return (
    <ContextMenu>
      <ContextMenuTrigger className="block h-full min-h-0 min-w-0">
        <PreviewLinesList
          lines={lines}
          isLoading={isLoading}
          wordWrap={isWordWrapEnabled}
          emptyText={emptyText}
        />
      </ContextMenuTrigger>
      <ContextMenuContent className="w-44">
        <ContextMenuCheckboxItem
          checked={isWordWrapEnabled}
          onCheckedChange={(checked) => {
            setIsWordWrapEnabled(checked === true);
          }}
        >
          Word Wrap
        </ContextMenuCheckboxItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
