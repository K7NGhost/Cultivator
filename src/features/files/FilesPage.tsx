import { useEffect, useLayoutEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertCircle, FolderOpen, Search } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { Separator } from "@/components/ui/separator";
import {
  type EvidenceDirectoryEntry,
  type EvidenceTreeNode,
  useEvidence,
} from "@/features/evidence/evidence-provider";
import { FileListViewer } from "@/features/files/components/FileListViewer";
import {
  FilePreviewViewer,
  type FilePreviewTab,
} from "@/features/files/components/FilePreviewViewer";
import { FileTreeViewer } from "@/features/files/components/FileTreeViewer";

export function FilesPage() {
  const { error, isLoading, listing, openDirectory } = useEvidence();
  const [selectedDirectory, setSelectedDirectory] =
    useState<EvidenceTreeNode | null>(null);
  const [directoryHistory, setDirectoryHistory] = useState<EvidenceTreeNode[]>(
    [],
  );
  const [visibleEntries, setVisibleEntries] = useState<EvidenceDirectoryEntry[]>(
    [],
  );
  const [selectedEntry, setSelectedEntry] =
    useState<EvidenceDirectoryEntry | null>(null);
  const [textPreview, setTextPreview] = useState<string[]>([]);
  const [hexPreview, setHexPreview] = useState<string[]>([]);
  const [entriesError, setEntriesError] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [isEntriesLoading, setIsEntriesLoading] = useState(false);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [activePreviewTab, setActivePreviewTab] =
    useState<FilePreviewTab>("text");

  useLayoutEffect(() => {
    setSelectedDirectory(listing?.tree ?? null);
    setVisibleEntries(listing?.entries ?? []);
    setSelectedEntry(listing?.entries[0] ?? null);
    setDirectoryHistory([]);
    setEntriesError(null);
    setPreviewError(null);
  }, [listing]);

  useEffect(() => {
    if (!selectedEntry || selectedEntry.kind !== "file") {
      setTextPreview([]);
      setHexPreview([]);
      setPreviewError(null);
      setIsPreviewLoading(false);
      return;
    }

    let isCurrent = true;
    setIsPreviewLoading(true);
    setPreviewError(null);
    setTextPreview([]);
    setHexPreview([]);

    const textPreviewRequest = invoke<string[]>("read_text_preview", {
      path: selectedEntry.path,
      line: 1,
    })
      .then((nextTextPreview) => {
        if (!isCurrent) {
          return;
        }

        setTextPreview(nextTextPreview);
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setPreviewError(
          caughtError instanceof Error
            ? caughtError.message
            : String(caughtError),
        );
        setTextPreview([]);
      });

    textPreviewRequest.finally(() => {
      if (isCurrent) {
        setIsPreviewLoading(false);
      }
    });

    return () => {
      isCurrent = false;
    };
  }, [selectedEntry]);

  useEffect(() => {
    if (
      activePreviewTab !== "hex" ||
      !selectedEntry ||
      selectedEntry.kind !== "file"
    ) {
      return;
    }

    let isCurrent = true;
    setIsPreviewLoading(true);
    setPreviewError(null);
    setHexPreview([]);

    invoke<string[]>("read_hex_file", {
      path: selectedEntry.path,
    })
      .then((nextHexPreview) => {
        if (!isCurrent) {
          return;
        }

        setHexPreview(nextHexPreview);
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setPreviewError(
          caughtError instanceof Error
            ? caughtError.message
            : String(caughtError),
        );
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
  }, [activePreviewTab, selectedEntry]);

  async function loadDirectoryEntries(
    node: EvidenceTreeNode,
    options: { pushHistory?: boolean } = {},
  ) {
    if (selectedDirectory?.path === node.path && options.pushHistory) {
      return;
    }

    if (options.pushHistory && selectedDirectory) {
      setDirectoryHistory((history) => [...history, selectedDirectory]);
    }

    setSelectedDirectory(node);
    setEntriesError(null);
    setIsEntriesLoading(true);

    try {
      const entries = await invoke<EvidenceDirectoryEntry[]>(
        "list_directory_entries",
        { path: node.path },
      );
      setVisibleEntries(entries);
      setSelectedEntry(entries[0] ?? null);
    } catch (caughtError) {
      setEntriesError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
      setVisibleEntries([]);
      setSelectedEntry(null);
    } finally {
      setIsEntriesLoading(false);
    }
  }

  function openFolderEntry(entry: EvidenceDirectoryEntry) {
    void loadDirectoryEntries(
      {
        id: entry.id,
        name: entry.name,
        path: entry.path,
        kind: entry.kind,
        files: entry.childCount ?? 0,
      },
      { pushHistory: true },
    );
  }

  function goBackDirectory() {
    const previousDirectory = directoryHistory[directoryHistory.length - 1];

    if (!previousDirectory) {
      return;
    }

    setDirectoryHistory((history) => history.slice(0, -1));
    void loadDirectoryEntries(previousDirectory);
  }

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <Button
          size="xs"
          className="h-7 px-2 text-xs"
          disabled={isLoading}
          onClick={() => {
            void openDirectory();
          }}
        >
          <FolderOpen className="size-3.5" aria-hidden="true" />
          {isLoading ? "Opening..." : "Open Directory"}
        </Button>
        <Separator orientation="vertical" className="h-5" />
        <div className="relative w-72">
          <Search
            className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            className="h-7 pl-7 text-xs"
            placeholder="Search logical files..."
          />
        </div>
        <div className="ml-auto flex items-center gap-1 text-[11px] text-muted-foreground">
          <span>{listing ? "1 directory mounted" : "0 directories mounted"}</span>
          <Separator orientation="vertical" className="h-4" />
          <span>{visibleEntries.length} visible entries</span>
        </div>
      </section>

      {(error || entriesError || previewError) && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{error ?? entriesError ?? previewError}</span>
        </section>
      )}

      <ResizablePanelGroup
        orientation="horizontal"
        className="min-h-0 flex-1"
      >
        <ResizablePanel
          defaultSize="24%"
          minSize="14%"
          maxSize="42%"
          className="min-h-0 min-w-0 overflow-hidden"
        >
          <FileTreeViewer
            rootNode={listing?.tree ?? null}
            selectedDirectory={selectedDirectory}
            onSelectDirectory={(node) => {
              void loadDirectoryEntries(node, { pushHistory: true });
            }}
          />
        </ResizablePanel>

        <ResizableHandle withHandle />

        <ResizablePanel
          defaultSize="76%"
          minSize="48%"
          className="min-h-0 min-w-0 overflow-hidden"
        >
          <ResizablePanelGroup
            orientation="vertical"
            className="h-full min-h-0 min-w-0"
          >
            <ResizablePanel
              defaultSize="62%"
              minSize="28%"
              className="min-h-0 min-w-0 overflow-hidden"
            >
              <FileListViewer
                entries={visibleEntries}
                isLoading={isEntriesLoading}
                selectedEntry={selectedEntry}
                canGoBack={directoryHistory.length > 0}
                onGoBack={goBackDirectory}
                onOpenFolder={openFolderEntry}
                onSelectEntry={setSelectedEntry}
              />
            </ResizablePanel>

            <ResizableHandle withHandle />

            <ResizablePanel
              defaultSize="38%"
              minSize="22%"
              className="min-h-0 min-w-0 overflow-hidden"
            >
              <FilePreviewViewer
                activeTab={activePreviewTab}
                hexPreview={hexPreview}
                isLoading={isPreviewLoading}
                onActiveTabChange={setActivePreviewTab}
                selectedEntry={selectedEntry}
                textPreview={textPreview}
              />
            </ResizablePanel>
          </ResizablePanelGroup>
        </ResizablePanel>
      </ResizablePanelGroup>

      <footer className="flex h-6 shrink-0 items-center gap-3 border-t px-2 text-[11px] text-muted-foreground">
        <span>{isLoading || isEntriesLoading ? "Loading" : "Ready"}</span>
        <span>Evidence: {listing?.rootName ?? "none"}</span>
        <span>Folder: {selectedDirectory?.name ?? "none"}</span>
        <span>Files indexed: {visibleEntries.length}</span>
        <span>Plugin jobs: 0 running</span>
      </footer>
    </div>
  );
}
