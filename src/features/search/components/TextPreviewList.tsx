import { useEffect, useMemo, useRef } from "react";
import { CellMeasurer, CellMeasurerCache, List, type ListRowProps } from "react-virtualized";
import "react-virtualized/styles.css";
import { cn } from "@/lib/utils";
import type { SearchMatch } from "../model/types";

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

export function TextPreviewList({
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

