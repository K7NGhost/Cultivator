import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

type HexPreviewViewerProps = {
  emptyText: string;
  path: string | null;
};

type HexPreviewSummary = {
  path: string;
  fileSize: number;
  pageCount: number;
  pageSize: number;
};

type HexPreviewPage = {
  pageIndex: number;
  pageNumber: number;
  byteOffset: number;
  byteLength: number;
  lines: string[];
};

function formatByteCount(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatOffset(offset: number) {
  return `0x${offset.toString(16).toUpperCase().padStart(8, "0")}`;
}

function parseOffsetInput(input: string) {
  const trimmedInput = input.trim();

  if (!trimmedInput) {
    return null;
  }

  const sign = trimmedInput.startsWith("-") ? -1 : 1;
  const unsignedInput =
    trimmedInput.startsWith("+") || trimmedInput.startsWith("-")
      ? trimmedInput.slice(1)
      : trimmedInput;
  const radix = unsignedInput.toLowerCase().startsWith("0x") ? 16 : 10;
  const parsedValue = Number.parseInt(unsignedInput, radix);

  if (!Number.isFinite(parsedValue)) {
    return null;
  }

  return {
    isRelative: trimmedInput.startsWith("+") || trimmedInput.startsWith("-"),
    value: parsedValue * sign,
  };
}

function getCaretLineOffset(textArea: HTMLTextAreaElement | null) {
  if (!textArea) {
    return null;
  }

  const textBeforeCaret = textArea.value.slice(0, textArea.selectionStart);
  const lineStart = textBeforeCaret.lastIndexOf("\n") + 1;
  const lineEnd = textArea.value.indexOf("\n", lineStart);
  const line = textArea.value.slice(
    lineStart,
    lineEnd === -1 ? textArea.value.length : lineEnd,
  );
  const offsetMatch = line.match(/^0x([0-9a-fA-F]+):/);

  if (!offsetMatch?.[1]) {
    return null;
  }

  return Number.parseInt(offsetMatch[1], 16);
}

export function HexPreviewViewer({ emptyText, path }: HexPreviewViewerProps) {
  const summaryRequestRef = useRef("");
  const pageRequestRef = useRef("");
  const textAreaRef = useRef<HTMLTextAreaElement | null>(null);
  const pendingOffsetRef = useRef<number | null>(null);
  const [summary, setSummary] = useState<HexPreviewSummary | null>(null);
  const [summarySourcePath, setSummarySourcePath] = useState<string | null>(null);
  const [page, setPage] = useState<HexPreviewPage | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [pageInput, setPageInput] = useState("1");
  const [offsetInput, setOffsetInput] = useState("");
  const [isSummaryLoading, setIsSummaryLoading] = useState(false);
  const [isPageLoading, setIsPageLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setSummary(null);
    setSummarySourcePath(null);
    setPage(null);
    setPageIndex(0);
    setPageInput("1");
    setOffsetInput("");
    setError(null);
    summaryRequestRef.current = "";
    pageRequestRef.current = "";
    pendingOffsetRef.current = null;

    if (!path) {
      setIsSummaryLoading(false);
      setIsPageLoading(false);
      return;
    }

    const requestKey = `${path}:summary`;
    summaryRequestRef.current = requestKey;
    setIsSummaryLoading(true);
    setIsPageLoading(false);

    invoke<HexPreviewSummary>("open_hex_preview", { path })
      .then((nextSummary) => {
        if (summaryRequestRef.current !== requestKey) {
          return;
        }

        setSummary(nextSummary);
        setSummarySourcePath(path);
      })
      .catch((caughtError) => {
        if (summaryRequestRef.current !== requestKey) {
          return;
        }

        setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
      })
      .finally(() => {
        if (summaryRequestRef.current === requestKey) {
          setIsSummaryLoading(false);
        }
      });
  }, [path]);

  useEffect(() => {
    if (!path || !summary || summarySourcePath !== path || summary.pageCount === 0) {
      return;
    }

    const requestKey = `${path}:page:${pageIndex}`;
    pageRequestRef.current = requestKey;
    setIsPageLoading(true);
    setError(null);

    // Autopsy's hex viewer loads one 16 KiB page into a text area. The Rust
    // command mirrors that behavior and formats the page with the same offset,
    // grouped hex, and ASCII columns.
    invoke<HexPreviewPage>("read_hex_preview_page", {
      path,
      pageIndex,
    })
      .then((nextPage) => {
        if (pageRequestRef.current !== requestKey) {
          return;
        }

        setPage(nextPage);
        setPageInput(String(nextPage.pageNumber));
      })
      .catch((caughtError) => {
        if (pageRequestRef.current !== requestKey) {
          return;
        }

        setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
        setPage(null);
      })
      .finally(() => {
        if (pageRequestRef.current === requestKey) {
          setIsPageLoading(false);
        }
      });
  }, [pageIndex, path, summary, summarySourcePath]);

  useEffect(() => {
    const targetOffset = pendingOffsetRef.current;
    const textArea = textAreaRef.current;

    if (targetOffset === null || !page || !textArea) {
      return;
    }

    const linePrefix = `0x${targetOffset.toString(16).padStart(8, "0")}:`;
    const lineStart = textArea.value.toLowerCase().indexOf(linePrefix);

    if (lineStart >= 0) {
      textArea.focus();
      textArea.setSelectionRange(lineStart, lineStart + linePrefix.length);
    }

    pendingOffsetRef.current = null;
  }, [page]);

  const pageText = useMemo(() => page?.lines.join("\n") ?? "", [page]);
  const canGoPrevious = pageIndex > 0 && !isPageLoading;
  const canGoNext =
    Boolean(summary) && pageIndex + 1 < (summary?.pageCount ?? 0) && !isPageLoading;

  const goToPage = useCallback(
    (nextPageNumber: number) => {
      if (!summary || summary.pageCount === 0) {
        return;
      }

      if (!Number.isFinite(nextPageNumber)) {
        setPageInput(String(pageIndex + 1));
        return;
      }

      const boundedPageNumber = Math.min(
        Math.max(1, nextPageNumber),
        summary.pageCount,
      );

      setPageIndex(boundedPageNumber - 1);
      setPageInput(String(boundedPageNumber));
    },
    [pageIndex, summary],
  );

  const goToOffset = useCallback(() => {
    if (!summary || summary.pageCount === 0) {
      return;
    }

    const parsedOffset = parseOffsetInput(offsetInput);

    if (!parsedOffset) {
      setError(`Invalid offset: ${offsetInput}`);
      return;
    }

    const baseOffset = parsedOffset.isRelative
      ? (getCaretLineOffset(textAreaRef.current) ?? page?.byteOffset ?? 0)
      : 0;
    const targetOffset = baseOffset + parsedOffset.value;

    if (targetOffset < 0 || targetOffset >= summary.fileSize) {
      setError(`Offset is outside the file: ${offsetInput}`);
      return;
    }

    pendingOffsetRef.current = targetOffset - (targetOffset % 16);
    const targetPageIndex = Math.floor(targetOffset / summary.pageSize);

    setError(null);
    setPageIndex(targetPageIndex);
    setPageInput(String(targetPageIndex + 1));
  }, [offsetInput, page?.byteOffset, summary]);

  if (!path) {
    return <div className="p-2 font-mono text-xs text-muted-foreground">{emptyText}</div>;
  }

  if (isSummaryLoading && !summary) {
    return (
      <div className="p-2 font-mono text-xs text-muted-foreground">
        Opening hex view...
      </div>
    );
  }

  if (error && !summary) {
    return <div className="p-2 font-mono text-xs text-destructive">{error}</div>;
  }

  if (!summary || summary.pageCount === 0) {
    return (
      <div className="p-2 font-mono text-xs text-muted-foreground">
        No hex preview available.
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col font-mono text-xs">
      <div className="flex h-8 shrink-0 items-center gap-2 overflow-x-auto border-b bg-muted/20 px-2 text-[11px]">
        <span className="shrink-0 text-muted-foreground">Page</span>
        <span className="shrink-0 text-muted-foreground">
          {page?.pageNumber ?? pageIndex + 1} of {summary.pageCount.toLocaleString()}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-6 w-6 shrink-0 rounded-sm"
          disabled={!canGoPrevious}
          onClick={() => goToPage(pageIndex)}
          title="Previous page"
        >
          <ChevronLeft className="size-3.5" aria-hidden="true" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-6 w-6 shrink-0 rounded-sm"
          disabled={!canGoNext}
          onClick={() => goToPage(pageIndex + 2)}
          title="Next page"
        >
          <ChevronRight className="size-3.5" aria-hidden="true" />
        </Button>
        <form
          className="flex shrink-0 items-center gap-1"
          onSubmit={(event) => {
            event.preventDefault();
            goToPage(Number.parseInt(pageInput, 10));
          }}
        >
          <span className="text-muted-foreground">Go to page</span>
          <Input
            aria-label="Go to hex page"
            className="h-6 w-16 rounded-sm px-1.5 py-0 text-center font-mono text-[11px]"
            inputMode="numeric"
            value={pageInput}
            onChange={(event) => setPageInput(event.target.value)}
            onBlur={() => goToPage(Number.parseInt(pageInput, 10))}
          />
        </form>
        <form
          className="flex shrink-0 items-center gap-1"
          onSubmit={(event) => {
            event.preventDefault();
            goToOffset();
          }}
        >
          <span className="text-muted-foreground">Jump to offset</span>
          <Input
            aria-label="Jump to hex offset"
            className="h-6 w-24 rounded-sm px-1.5 py-0 font-mono text-[11px]"
            placeholder="0x0"
            value={offsetInput}
            onChange={(event) => setOffsetInput(event.target.value)}
          />
        </form>
        <span className="shrink-0 text-muted-foreground">
          {formatByteCount(summary.fileSize)}
        </span>
        <span className="shrink-0 text-muted-foreground">
          {page
            ? `${formatOffset(page.byteOffset)}-${formatOffset(
                page.byteOffset + page.byteLength,
              )}`
            : "Loading offset"}
        </span>
        <span className={error ? "truncate text-destructive" : "text-muted-foreground"}>
          {error ?? (isPageLoading ? "Loading hex..." : "Ready")}
        </span>
      </div>
      <textarea
        ref={textAreaRef}
        className="min-h-0 flex-1 resize-none overflow-auto border-0 bg-background p-2 font-mono text-xs leading-5 outline-none"
        readOnly
        spellCheck={false}
        value={isPageLoading && !page ? "Loading hex from file..." : pageText}
      />
    </div>
  );
}
