import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

type TextPreviewViewerProps = {
  emptyText: string;
  path: string | null;
};

type TextPreviewSummary = {
  path: string;
  fileSize: number;
  lineCount: number;
  sourceLineCount: number;
  longestLineWidth: number;
};

type TextPreviewLine = {
  rowIndex: number;
  lineNumber: number;
  byteOffset: number;
  byteLength: number;
  isContinuation: boolean;
  text: string;
};

function formatOffset(offset: number) {
  return `0x${offset.toString(16).toUpperCase().padStart(8, "0")}`;
}

function formatByteCount(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatAsciiText(text: string) {
  let formattedText = "";

  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    const character = text[index] ?? "";

    if (code === 10 || code === 9 || (code >= 32 && code <= 126)) {
      formattedText += character;
    } else if (code !== 13) {
      formattedText += ".";
    }
  }

  return formattedText;
}

export function TextPreviewViewer({ emptyText, path }: TextPreviewViewerProps) {
  const summaryRequestRef = useRef("");
  const pageRequestRef = useRef("");
  const [summary, setSummary] = useState<TextPreviewSummary | null>(null);
  const [summarySourcePath, setSummarySourcePath] = useState<string | null>(null);
  const [page, setPage] = useState<TextPreviewLine | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [pageInput, setPageInput] = useState("1");
  const [isSummaryLoading, setIsSummaryLoading] = useState(false);
  const [isPageLoading, setIsPageLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setSummary(null);
    setSummarySourcePath(null);
    setPage(null);
    setPageIndex(0);
    setPageInput("1");
    setError(null);
    summaryRequestRef.current = "";
    pageRequestRef.current = "";

    if (!path) {
      setIsSummaryLoading(false);
      setIsPageLoading(false);
      return;
    }

    const requestKey = `${path}:summary`;
    summaryRequestRef.current = requestKey;
    setIsSummaryLoading(true);
    setIsPageLoading(false);

    invoke<TextPreviewSummary>("open_text_preview", { path })
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
    if (!path || !summary || summarySourcePath !== path || summary.lineCount === 0) {
      return;
    }

    const requestKey = `${path}:page:${pageIndex}`;
    pageRequestRef.current = requestKey;
    setIsPageLoading(true);
    setError(null);

    // Autopsy reads one 16 KiB page at a time and extracts strings from that
    // page. Cultivator keeps the same lazy page model with a cached mmap behind
    // this command, so changing pages does not scan or allocate the whole file.
    invoke<TextPreviewLine[]>("read_text_preview_lines", {
      path,
      startLine: pageIndex,
      lineCount: 1,
    })
      .then((rows) => {
        if (pageRequestRef.current !== requestKey) {
          return;
        }

        setPage(rows[0] ?? null);
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

  const pageCount = summary?.lineCount ?? 0;
  const pageText = useMemo(() => formatAsciiText(page?.text ?? ""), [page]);
  const canGoPrevious = pageIndex > 0 && !isPageLoading;
  const canGoNext = pageCount > 0 && pageIndex + 1 < pageCount && !isPageLoading;

  const goToPage = useCallback(
    (nextPageNumber: number) => {
      if (!summary || summary.lineCount === 0) {
        return;
      }

      if (!Number.isFinite(nextPageNumber)) {
        setPageInput(String(pageIndex + 1));
        return;
      }

      const boundedPageNumber = Math.min(
        Math.max(1, nextPageNumber),
        summary.lineCount,
      );

      setPageIndex(boundedPageNumber - 1);
      setPageInput(String(boundedPageNumber));
    },
    [pageIndex, summary],
  );

  if (!path) {
    return <div className="p-2 font-mono text-xs text-muted-foreground">{emptyText}</div>;
  }

  if (isSummaryLoading && !summary) {
    return (
      <div className="p-2 font-mono text-xs text-muted-foreground">
        Opening text map...
      </div>
    );
  }

  if (error && !summary) {
    return <div className="p-2 font-mono text-xs text-destructive">{error}</div>;
  }

  if (!summary || summary.lineCount === 0) {
    return (
      <div className="p-2 font-mono text-xs text-muted-foreground">
        No text preview available.
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col font-mono text-xs">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b bg-muted/20 px-2 text-[11px]">
        <span className="text-muted-foreground">Page</span>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-6 w-6 rounded-sm"
          disabled={!canGoPrevious}
          onClick={() => goToPage(pageIndex)}
          title="Previous page"
        >
          <ChevronLeft className="size-3.5" aria-hidden="true" />
        </Button>
        <form
          className="flex items-center gap-1"
          onSubmit={(event) => {
            event.preventDefault();
            goToPage(Number.parseInt(pageInput, 10));
          }}
        >
          <Input
            aria-label="Go to text preview page"
            className="h-6 w-16 rounded-sm px-1.5 py-0 text-center font-mono text-[11px]"
            inputMode="numeric"
            value={pageInput}
            onChange={(event) => setPageInput(event.target.value)}
            onBlur={() => goToPage(Number.parseInt(pageInput, 10))}
          />
          <span className="text-muted-foreground">
            of {pageCount.toLocaleString()}
          </span>
        </form>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-6 w-6 rounded-sm"
          disabled={!canGoNext}
          onClick={() => goToPage(pageIndex + 2)}
          title="Next page"
        >
          <ChevronRight className="size-3.5" aria-hidden="true" />
        </Button>
        <span className="ml-2 text-muted-foreground">
          {formatByteCount(summary.fileSize)}
        </span>
        <span className="text-muted-foreground">
          {page
            ? `${formatOffset(page.byteOffset)}-${formatOffset(
                page.byteOffset + page.byteLength,
              )}`
            : "Loading offset"}
        </span>
        <span className={error ? "truncate text-destructive" : "text-muted-foreground"}>
          {error ?? (isPageLoading ? "Loading text..." : "ASCII strings")}
        </span>
      </div>
      <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap p-2 font-mono text-xs leading-5">
        {isPageLoading && !page ? "Loading text..." : pageText}
      </pre>
    </div>
  );
}
