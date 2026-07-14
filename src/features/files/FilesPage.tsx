import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertCircle, Plus, Play, Search } from "lucide-react";

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
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { Separator } from "@/components/ui/separator";
import { useCases } from "@/features/cases/case-provider";
import {
  listDataSources,
  notifyDataSourcesChanged,
  removeDataSource,
  subscribeToDataSourcesChanged,
} from "@/features/datasources/dataSourceRepository";
import { DataSourceWizardDialog } from "@/features/datasources/DataSourceWizardDialog";
import type {
  DataSourcePluginTarget,
  DataSourceRecord,
} from "@/features/datasources/types";
import {
  type EvidenceDirectoryEntry,
  type EvidenceDirectoryListing,
  type EvidenceTreeNode,
  useEvidence,
} from "@/features/evidence/evidence-provider";
import { FileListViewer } from "@/features/files/components/FileListViewer";
import {
  FilePreviewViewer,
  type FileFormatPreview,
  type FilePreviewTab,
} from "@/features/files/components/FilePreviewViewer";
import {
  FileTreeViewer,
  type FileViewSelection,
} from "@/features/files/components/FileTreeViewer";
import {
  listFileTagsForPaths,
  listFileTagSummaries,
  listTaggedFileEntries,
  removeFileTag,
  upsertFileTag,
  type FileTagRecord,
  type FileTagSummary,
} from "@/features/files/fileTagRepository";
import {
  cancelDatasourcePluginRun,
  listPythonPlugins,
  pluginRunUpdatedDatasourcePaths,
  runDatasourcePlugins,
} from "@/features/plugins/pluginRepository";
import {
  createPluginRunId,
  showPluginRunFailedToast,
  showPluginRunFinishedToasts,
  showPluginRunStartedToast,
} from "@/features/plugins/pluginToasts";
import type { PythonPlugin } from "@/features/plugins/types";
import { cn } from "@/lib/utils";

function getPluginTargetLabel(target: PythonPlugin["target"]) {
  switch (target) {
    case "android":
      return "Android";
    case "ios":
      return "iOS";
    case "windows":
      return "Windows";
    case "macos":
      return "macOS";
    case "infotainment":
      return "Infotainment";
    case "other":
      return "Other";
  }
}

const pluginTargetFilters: Array<{
  target: DataSourcePluginTarget | "all";
  label: string;
}> = [
  { target: "all", label: "All" },
  { target: "infotainment", label: "Infotainment" },
  { target: "android", label: "Android" },
  { target: "ios", label: "iOS" },
  { target: "windows", label: "Windows" },
  { target: "macos", label: "macOS" },
  { target: "other", label: "Other" },
];

type FileViewIndex = {
  entries: Record<string, EvidenceDirectoryEntry[]>;
  counts: Record<string, number>;
};

const FILE_VIEW_PAGE_SIZE = 10_000;
type FileViewPageInfo = {
  offset: number;
  limit: number;
  totalCount: number;
  hasNextPage: boolean;
};

type FileViewEntriesPage = FileViewPageInfo & {
  entries: EvidenceDirectoryEntry[];
};

function createFileViewPageInfo(
  view: FileViewSelection,
  entriesById: Record<string, EvidenceDirectoryEntry[]>,
  counts: Record<string, number>,
): FileViewPageInfo | null {
  if (view.childViews?.length) {
    return null;
  }

  const entries = entriesById[view.id] ?? [];
  const totalCount = counts[view.id] ?? view.count ?? entries.length;

  return {
    offset: 0,
    limit: FILE_VIEW_PAGE_SIZE,
    totalCount,
    hasNextPage: entries.length < totalCount,
  };
}

export function FilesPage() {
  const { error, isLoading, listing } = useEvidence();
  const { activeCase } = useCases();
  const [selectedDirectory, setSelectedDirectory] =
    useState<EvidenceTreeNode | null>(null);
  const [selectedFileView, setSelectedFileView] =
    useState<FileViewSelection | null>(null);
  const selectedFileViewRef = useRef<FileViewSelection | null>(null);
  const [fileViewEntriesById, setFileViewEntriesById] = useState<
    Record<string, EvidenceDirectoryEntry[]>
  >({});
  const [fileViewCounts, setFileViewCounts] = useState<Record<string, number>>(
    {},
  );
  const [fileTagsByPath, setFileTagsByPath] = useState<
    Record<string, FileTagRecord[]>
  >({});
  const [fileTagSummaries, setFileTagSummaries] = useState<FileTagSummary[]>([]);
  const [fileTagRefreshKey, setFileTagRefreshKey] = useState(0);
  const [directoryHistory, setDirectoryHistory] = useState<EvidenceTreeNode[]>(
    [],
  );
  const [visibleEntries, setVisibleEntries] = useState<EvidenceDirectoryEntry[]>(
    [],
  );
  const [selectedEntry, setSelectedEntry] =
    useState<EvidenceDirectoryEntry | null>(null);
  const [filePreview, setFilePreview] = useState<FileFormatPreview | null>(null);
  const [entriesError, setEntriesError] = useState<string | null>(null);
  const [dataSourceError, setDataSourceError] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [dataSourceTreeNodes, setDataSourceTreeNodes] = useState<
    EvidenceTreeNode[]
  >([]);
  const [dataSources, setDataSources] = useState<DataSourceRecord[]>([]);
  const [fileViewDataSourceId, setFileViewDataSourceId] = useState("all");
  const [pluginRunDataSource, setPluginRunDataSource] =
    useState<DataSourceRecord | null>(null);
  const [availablePlugins, setAvailablePlugins] = useState<PythonPlugin[]>([]);
  const [selectedRunPluginIds, setSelectedRunPluginIds] = useState<string[]>([]);
  const [pluginOptionValues, setPluginOptionValues] = useState<
    Record<string, Record<string, string>>
  >({});
  const [activeRunPluginId, setActiveRunPluginId] = useState("");
  const [pluginFilter, setPluginFilter] = useState("");
  const [pluginTargetFilter, setPluginTargetFilter] = useState<
    DataSourcePluginTarget | "all"
  >("all");
  const [isDataSourcesLoading, setIsDataSourcesLoading] = useState(false);
  const [isEntriesLoading, setIsEntriesLoading] = useState(false);
  const [isFileViewPageLoading, setIsFileViewPageLoading] = useState(false);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [isPluginsLoading, setIsPluginsLoading] = useState(false);
  const [isRunningPlugins, setIsRunningPlugins] = useState(false);
  const [isDataSourceWizardOpen, setIsDataSourceWizardOpen] = useState(false);
  const [dataSourceRefreshKey, setDataSourceRefreshKey] = useState(0);
  const [activePreviewTab, setActivePreviewTab] =
    useState<FilePreviewTab>("text");
  const [selectedFileViewPageInfo, setSelectedFileViewPageInfo] =
    useState<FileViewPageInfo | null>(null);
  const treeRootNodes = useMemo(() => {
    return [...dataSourceTreeNodes, ...(listing?.tree ? [listing.tree] : [])];
  }, [dataSourceTreeNodes, listing?.tree]);
  const fileViewRoots = useMemo(() => {
    if (fileViewDataSourceId !== "all") {
      return (
        dataSources.find((dataSource) => dataSource.id === fileViewDataSourceId)
          ?.paths ?? []
      );
    }

    const roots = [
      ...dataSources.flatMap((dataSource) => dataSource.paths),
      ...(listing?.rootPath ? [listing.rootPath] : []),
    ];

    return Array.from(new Set(roots));
  }, [dataSources, fileViewDataSourceId, listing?.rootPath]);
  const visiblePlugins = useMemo(() => {
    const normalizedFilter = pluginFilter.trim().toLowerCase();

    const targetMatches = (plugin: PythonPlugin) =>
      pluginTargetFilter === "all" || plugin.target === pluginTargetFilter;

    if (!normalizedFilter) {
      return availablePlugins.filter(targetMatches);
    }

    return availablePlugins.filter((plugin) => {
      if (!targetMatches(plugin)) {
        return false;
      }

      return (
        plugin.name.toLowerCase().includes(normalizedFilter) ||
        plugin.type.toLowerCase().includes(normalizedFilter) ||
        plugin.description.toLowerCase().includes(normalizedFilter) ||
        plugin.id.toLowerCase().includes(normalizedFilter)
      );
    });
  }, [availablePlugins, pluginFilter, pluginTargetFilter]);
  const activeRunPlugin =
    visiblePlugins.find((plugin) => plugin.id === activeRunPluginId) ??
    visiblePlugins[0] ??
    null;
  const pluginMap = useMemo(() => {
    return new Map(availablePlugins.map((plugin) => [plugin.id, plugin]));
  }, [availablePlugins]);

  useLayoutEffect(() => {
    setSelectedDirectory(listing?.tree ?? null);
    setVisibleEntries(listing?.entries ?? []);
    setSelectedEntry(listing?.entries[0] ?? null);
    setSelectedFileView(null);
    selectedFileViewRef.current = null;
    setSelectedFileViewPageInfo(null);
    setDirectoryHistory([]);
    setEntriesError(null);
    setPreviewError(null);
  }, [listing]);

  useEffect(() => {
    if (!activeCase) {
      setDataSources([]);
      setFileViewDataSourceId("all");
      setDataSourceTreeNodes([]);
      setDataSourceError(null);
      setIsDataSourcesLoading(false);
      return;
    }

    let isCurrent = true;
    setIsDataSourcesLoading(true);
    setDataSourceError(null);

    listDataSources(activeCase.databasePath, activeCase.id)
      .then(async (dataSources) => {
        const nextNodes = await Promise.all(
          dataSources.map((dataSource) => buildDataSourceTreeNode(dataSource)),
        );

        if (!isCurrent) {
          return;
        }

        setDataSources(dataSources);
        setFileViewDataSourceId((currentId) =>
          currentId === "all" ||
          dataSources.some((dataSource) => dataSource.id === currentId)
            ? currentId
            : "all",
        );
        setDataSourceTreeNodes(nextNodes);
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
        setDataSourceTreeNodes([]);
      })
      .finally(() => {
        if (isCurrent) {
          setIsDataSourcesLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase, dataSourceRefreshKey]);

  useEffect(() => {
    if (!activeCase) {
      return;
    }

    return subscribeToDataSourcesChanged((caseId) => {
      if (caseId === activeCase.id) {
        setDataSourceRefreshKey((currentKey) => currentKey + 1);
      }
    });
  }, [activeCase]);

  useEffect(() => {
    if (listing || selectedDirectory || selectedFileView || !dataSourceTreeNodes[0]) {
      return;
    }

    selectDataSourceRoot(dataSourceTreeNodes[0]);
  }, [dataSourceTreeNodes, listing, selectedDirectory, selectedFileView]);

  useEffect(() => {
    if (!activeCase) {
      setFileTagsByPath({});
      return;
    }

    const visiblePaths = visibleEntries.map((entry) => entry.path);

    if (visiblePaths.length === 0) {
      setFileTagsByPath({});
      return;
    }

    let isCurrent = true;

    listFileTagsForPaths(activeCase.databasePath, visiblePaths)
      .then((tagsByPath) => {
        if (isCurrent) {
          setFileTagsByPath(tagsByPath);
        }
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setEntriesError(
          caughtError instanceof Error
            ? caughtError.message
            : String(caughtError),
        );
        setFileTagsByPath({});
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase, fileTagRefreshKey, visibleEntries]);

  useEffect(() => {
    if (!activeCase) {
      setFileTagSummaries([]);
      return;
    }

    let isCurrent = true;

    listFileTagSummaries(activeCase.databasePath)
      .then((summaries) => {
        if (isCurrent) {
          setFileTagSummaries(summaries);
        }
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setEntriesError(
          caughtError instanceof Error
            ? caughtError.message
            : String(caughtError),
        );
        setFileTagSummaries([]);
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase, fileTagRefreshKey]);

  useEffect(() => {
    selectedFileViewRef.current = selectedFileView;
  }, [selectedFileView]);

  function clearFileViewPageLoadingAfterPaint(viewId: string) {
    window.requestAnimationFrame(() => {
      if (selectedFileViewRef.current?.id === viewId) {
        setIsFileViewPageLoading(false);
      }
    });
  }

  function clearFileViewIndexLoadingAfterPaint(viewId: string) {
    window.requestAnimationFrame(() => {
      if (selectedFileViewRef.current?.id === viewId) {
        setIsEntriesLoading(false);
      }
    });
  }

  useEffect(() => {
    if (fileViewRoots.length === 0) {
      setFileViewEntriesById({});
      setFileViewCounts({});
      setSelectedFileViewPageInfo(null);
      setIsFileViewPageLoading(false);
      return;
    }

    let isCurrent = true;
    let deferredSelectedViewRenderId: string | null = null;
    setEntriesError(null);
    if (selectedFileViewRef.current) {
      setIsEntriesLoading(true);
    }

    invoke<FileViewIndex>("build_file_view_index", {
      roots: fileViewRoots,
    })
      .then((index) => {
        if (!isCurrent) {
          return;
        }

        setFileViewEntriesById(index.entries);
        setFileViewCounts(index.counts);
        const currentView = selectedFileViewRef.current;

        if (currentView) {
          const refreshedView = applyFileViewCounts(currentView, index.counts);
          setSelectedFileView(refreshedView);
          selectedFileViewRef.current = refreshedView;

          setSelectedFileViewPageInfo(
            createFileViewPageInfo(refreshedView, index.entries, index.counts),
          );

          if (refreshedView.childViews?.length) {
            const entries = refreshedView.childViews.map(fileViewToDirectoryEntry);
            setVisibleEntries(entries);
            setSelectedEntry(entries[0] ?? null);
          } else {
            const entries = index.entries[refreshedView.id] ?? [];
            deferredSelectedViewRenderId = refreshedView.id;

            window.requestAnimationFrame(() => {
              if (!isCurrent || selectedFileViewRef.current?.id !== refreshedView.id) {
                return;
              }

              // Large file-view pages can contain 10,000 rows. Defer attaching
              // them until after the loading overlay has rendered, then clear the
              // overlay on the next paint so the UI does not feel stuck on click.
              setVisibleEntries(entries);
              setSelectedEntry(entries[0] ?? null);
              clearFileViewIndexLoadingAfterPaint(refreshedView.id);
            });
          }
        }
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setFileViewEntriesById({});
        setFileViewCounts({});
        setSelectedFileViewPageInfo(null);
        setEntriesError(
          caughtError instanceof Error
            ? caughtError.message
            : String(caughtError),
        );
      })
      .finally(() => {
        if (isCurrent) {
          if (
            selectedFileViewRef.current &&
            selectedFileViewRef.current.id !== deferredSelectedViewRenderId
          ) {
            setIsEntriesLoading(false);
          }
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [fileViewRoots]);

  useEffect(() => {
    if (!selectedEntry || selectedEntry.kind !== "file") {
      setFilePreview(null);
      setPreviewError(null);
      setIsPreviewLoading(false);
      return;
    }

    let isCurrent = true;
    setIsPreviewLoading(true);
    setPreviewError(null);
    setFilePreview(null);

    const filePreviewRequest = invoke<FileFormatPreview | null>(
      "read_file_format_preview",
      {
        path: selectedEntry.path,
      },
    )
      .then((nextFilePreview) => {
        if (!isCurrent) {
          return;
        }

        setFilePreview(nextFilePreview);
        setActivePreviewTab((currentTab) => {
          if (nextFilePreview) {
            return "file";
          }

          return currentTab === "file" ? "text" : currentTab;
        });
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
        setFilePreview(null);
      });

    Promise.allSettled([filePreviewRequest]).finally(() => {
      if (isCurrent) {
        setIsPreviewLoading(false);
      }
    });

    return () => {
      isCurrent = false;
    };
  }, [selectedEntry]);

  async function loadDirectoryEntries(
    node: EvidenceTreeNode,
    options: { pushHistory?: boolean } = {},
  ) {
    if (node.kind !== "directory") {
      return;
    }

    if (selectedDirectory?.id === node.id && options.pushHistory) {
      return;
    }

    if (options.pushHistory && selectedDirectory) {
      setDirectoryHistory((history) => [...history, selectedDirectory]);
    }

    setSelectedFileView(null);
    selectedFileViewRef.current = null;
    setSelectedFileViewPageInfo(null);
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
    const fileViewId = entry.id.startsWith("file-view:")
      ? entry.id.slice("file-view:".length)
      : null;

    if (fileViewId && selectedFileView?.childViews) {
      const childView = selectedFileView.childViews.find(
        (view) => view.id === fileViewId,
      );

      if (childView) {
        selectFileView(childView);
        return;
      }
    }

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
    setSelectedFileView(null);
    selectedFileViewRef.current = null;
    setSelectedFileViewPageInfo(null);

    if (previousDirectory.kind === "datasource") {
      selectDataSourceRoot(previousDirectory);
      return;
    }

    void loadDirectoryEntries(previousDirectory);
  }

  function selectDataSourceRoot(
    node: EvidenceTreeNode,
    options: { pushHistory?: boolean } = {},
  ) {
    if (options.pushHistory && selectedDirectory?.id !== node.id) {
      setDirectoryHistory((history) =>
        selectedDirectory ? [...history, selectedDirectory] : history,
      );
    }

    const entries = (node.children ?? []).map(treeNodeToDirectoryEntry);

    setSelectedFileView(null);
    selectedFileViewRef.current = null;
    setSelectedFileViewPageInfo(null);
    setSelectedDirectory(node);
    setVisibleEntries(entries);
    setSelectedEntry(entries[0] ?? null);
    setEntriesError(null);
    setPreviewError(null);
  }

  function selectTreeNode(node: EvidenceTreeNode) {
    setSelectedFileView(null);
    selectedFileViewRef.current = null;
    setSelectedFileViewPageInfo(null);

    if (node.kind === "datasource") {
      selectDataSourceRoot(node, { pushHistory: true });
      return;
    }

    if (node.kind === "file") {
      setSelectedEntry(treeNodeToDirectoryEntry(node));
      setPreviewError(null);
      return;
    }

    void loadDirectoryEntries(node, { pushHistory: true });
  }

  async function loadTaggedFileView(view: FileViewSelection) {
    if (!activeCase || !view.tagName) {
      setEntriesError("Open a case before using tag views.");
      return;
    }

    if (selectedDirectory) {
      setDirectoryHistory((history) => [...history, selectedDirectory]);
    }

    setSelectedFileView(view);
    selectedFileViewRef.current = view;
    setSelectedDirectory(null);
    setSelectedFileViewPageInfo(null);
    setVisibleEntries([]);
    setSelectedEntry(null);
    setEntriesError(null);
    setIsEntriesLoading(true);
    setIsFileViewPageLoading(false);

    try {
      const entries = await listTaggedFileEntries(
        activeCase.databasePath,
        view.tagName,
      );

      if (selectedFileViewRef.current?.id !== view.id) {
        return;
      }

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
      if (selectedFileViewRef.current?.id === view.id) {
        setIsEntriesLoading(false);
      }
    }
  }

  function selectFileView(view: FileViewSelection) {
    if (view.tagName) {
      void loadTaggedFileView(view);
      return;
    }

    if (fileViewRoots.length === 0) {
      setSelectedFileView(view);
      selectedFileViewRef.current = view;
      setSelectedDirectory(null);
      setSelectedFileViewPageInfo(null);
      setVisibleEntries([]);
      setSelectedEntry(null);
      setEntriesError("Add or open a data source before using file views.");
      return;
    }

    if (selectedDirectory) {
      setDirectoryHistory((history) => [...history, selectedDirectory]);
    }

    setSelectedFileView(view);
    selectedFileViewRef.current = view;
    setSelectedDirectory(null);
    setEntriesError(null);

    if (view.childViews?.length) {
      const entries = view.childViews.map(fileViewToDirectoryEntry);
      setIsEntriesLoading(false);
      setIsFileViewPageLoading(false);
      setSelectedFileViewPageInfo(null);
      setVisibleEntries(entries);
      setSelectedEntry(entries[0] ?? null);
      return;
    }

    setIsEntriesLoading(false);
    setIsFileViewPageLoading(true);
    setSelectedFileViewPageInfo(
      createFileViewPageInfo(view, fileViewEntriesById, fileViewCounts),
    );
    setVisibleEntries([]);
    setSelectedEntry(null);

    const cachedEntries = fileViewEntriesById[view.id];

    if (cachedEntries) {
      window.requestAnimationFrame(() => {
        if (selectedFileViewRef.current?.id !== view.id) {
          return;
        }

        // File-view leaf nodes use prebuilt 10,000-row pages. Rendering them on
        // the click stack blocks the spinner, so the page data is attached after
        // the loading state has had a frame to paint.
        setVisibleEntries(cachedEntries);
        setSelectedEntry(cachedEntries[0] ?? null);
        clearFileViewPageLoadingAfterPaint(view.id);
      });
      return;
    }

    void loadFileViewPage(0);
  }

  async function loadFileViewPage(offset: number) {
    const view = selectedFileViewRef.current;

    if (!view || view.childViews?.length || fileViewRoots.length === 0) {
      return;
    }

    const requestedOffset = Math.max(0, offset);

    setIsFileViewPageLoading(true);
    setEntriesError(null);
    let clearLoadingAfterPaint = false;

    try {
      const page = await invoke<FileViewEntriesPage>(
        "list_file_view_entries_page",
        {
          roots: fileViewRoots,
          viewId: view.id,
          offset: requestedOffset,
          limit: FILE_VIEW_PAGE_SIZE,
        },
      );

      if (selectedFileViewRef.current?.id !== view.id) {
        return;
      }

      setVisibleEntries(page.entries);
      setSelectedEntry(page.entries[0] ?? null);
      setFileViewCounts((counts) => ({
        ...counts,
        [view.id]: page.totalCount,
      }));
      setSelectedFileViewPageInfo({
        offset: page.offset,
        limit: page.limit,
        totalCount: page.totalCount,
        hasNextPage: page.hasNextPage,
      });
      clearLoadingAfterPaint = true;
    } catch (caughtError) {
      setEntriesError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
    } finally {
      if (selectedFileViewRef.current?.id === view.id) {
        if (clearLoadingAfterPaint) {
          clearFileViewPageLoadingAfterPaint(view.id);
        } else {
          setIsFileViewPageLoading(false);
        }
      }
    }
  }

  function loadPreviousFileViewPage() {
    if (!selectedFileViewPageInfo) {
      return;
    }

    void loadFileViewPage(
      Math.max(
        0,
        selectedFileViewPageInfo.offset - selectedFileViewPageInfo.limit,
      ),
    );
  }

  function loadNextFileViewPage() {
    if (!selectedFileViewPageInfo?.hasNextPage) {
      return;
    }

    void loadFileViewPage(
      selectedFileViewPageInfo.offset + selectedFileViewPageInfo.limit,
    );
  }

  async function handleSetFileTag(input: {
    entry: EvidenceDirectoryEntry;
    tagName: string;
    tagGroup: FileTagRecord["tagGroup"];
    comment?: string;
  }) {
    if (!activeCase) {
      setEntriesError("Open a case before tagging files.");
      return;
    }

    try {
      await upsertFileTag({
        caseDatabasePath: activeCase.databasePath,
        entry: input.entry,
        tagName: input.tagName,
        tagGroup: input.tagGroup,
        comment: input.comment,
      });
      setEntriesError(null);
      if (selectedFileViewRef.current?.tagName === input.tagName) {
        setVisibleEntries((currentEntries) => {
          if (currentEntries.some((entry) => entry.path === input.entry.path)) {
            return currentEntries;
          }

          return [...currentEntries, input.entry];
        });
      }
      setFileTagRefreshKey((currentKey) => currentKey + 1);
    } catch (caughtError) {
      setEntriesError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
    }
  }

  async function handleRemoveFileTag(
    entry: EvidenceDirectoryEntry,
    tagName: string,
  ) {
    if (!activeCase) {
      setEntriesError("Open a case before tagging files.");
      return;
    }

    try {
      await removeFileTag({
        caseDatabasePath: activeCase.databasePath,
        filePath: entry.path,
        tagName,
      });
      setEntriesError(null);
      if (selectedFileViewRef.current?.tagName === tagName) {
        setVisibleEntries((currentEntries) =>
          currentEntries.filter((currentEntry) => currentEntry.path !== entry.path),
        );
        setSelectedEntry((currentEntry) =>
          currentEntry?.path === entry.path ? null : currentEntry,
        );
      }
      setFileTagRefreshKey((currentKey) => currentKey + 1);
    } catch (caughtError) {
      setEntriesError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
    }
  }

  async function handleRemoveDataSource(node: EvidenceTreeNode) {
    if (!activeCase || node.kind !== "datasource") {
      return;
    }

    const dataSourceId = node.id.replace(/^datasource:/, "");

    setDataSourceError(null);

    try {
      await removeDataSource({
        caseDatabasePath: activeCase.databasePath,
        caseId: activeCase.id,
        dataSourceId,
      });

      if (selectedDirectory?.id === node.id) {
        setSelectedDirectory(null);
        setVisibleEntries([]);
        setSelectedEntry(null);
        setSelectedFileView(null);
        selectedFileViewRef.current = null;
        setDirectoryHistory([]);
      }
    } catch (caughtError) {
      setDataSourceError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
    }
  }

  function openRunPluginsDialog(node: EvidenceTreeNode) {
    if (!activeCase || node.kind !== "datasource") {
      return;
    }

    const dataSourceId = node.id.replace(/^datasource:/, "");
    const dataSource = dataSources.find(
      (currentDataSource) => currentDataSource.id === dataSourceId,
    );

    if (!dataSource) {
      setDataSourceError("Datasource was not found.");
      return;
    }

    setPluginRunDataSource(dataSource);
    setSelectedRunPluginIds(dataSource.pluginIds);
    setPluginOptionValues(
      Object.fromEntries(
        availablePlugins.map((plugin) => [
          plugin.id,
          Object.fromEntries(
            (plugin.options ?? []).map((option) => [
              option.id,
              option.defaultValue,
            ]),
          ),
        ]),
      ),
    );
    setPluginFilter("");
    setDataSourceError(null);
  }

  function toggleRunPlugin(plugin: PythonPlugin, isSelected: boolean) {
    setSelectedRunPluginIds((currentPluginIds) => {
      if (isSelected) {
        return currentPluginIds.includes(plugin.id)
          ? currentPluginIds
          : [...currentPluginIds, plugin.id];
      }

      return currentPluginIds.filter((pluginId) => pluginId !== plugin.id);
    });
  }

  async function runSelectedPlugins() {
    if (!activeCase || !pluginRunDataSource) {
      return;
    }

    const runDataSource = pluginRunDataSource;
    const runPluginIds = selectedRunPluginIds;

    if (runPluginIds.length === 0) {
      setDataSourceError("Select at least one plugin to run.");
      return;
    }

    setIsRunningPlugins(true);
    setDataSourceError(null);
    const runId = createPluginRunId();
    const toastId = showPluginRunStartedToast({
      datasourceName: runDataSource.name,
      jobs: runPluginIds.map((pluginId) => ({
        pluginId,
        pluginName: pluginMap.get(pluginId)?.name ?? pluginId,
      })),
      onCancel: async () => {
        await cancelDatasourcePluginRun(runId);
      },
      pluginCount: runPluginIds.length,
      runId,
    });
    setPluginRunDataSource(null);

    try {
      const summary = await runDatasourcePlugins({
        caseDatabasePath: activeCase.databasePath,
        caseFolderPath: activeCase.folderPath,
        datasourceId: runDataSource.id,
        pluginIds: runPluginIds,
        pluginOptions: pluginOptionValues,
        runId,
      });
      showPluginRunFinishedToasts({
        datasourceName: runDataSource.name,
        pluginMap,
        runId,
        summary,
        toastId,
      });
      if (pluginRunUpdatedDatasourcePaths(summary)) {
        notifyDataSourcesChanged(activeCase.id);
      }
    } catch (caughtError) {
      showPluginRunFailedToast({
        datasourceName: runDataSource.name,
        error: caughtError,
        runId,
        toastId,
      });
      setDataSourceError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
    } finally {
      setIsRunningPlugins(false);
    }
  }

  useEffect(() => {
    if (!pluginRunDataSource) {
      return;
    }

    let isCurrent = true;

    setIsPluginsLoading(true);
    listPythonPlugins()
      .then((plugins) => {
        if (!isCurrent) {
          return;
        }

        setAvailablePlugins(plugins);
        setSelectedRunPluginIds((currentPluginIds) => {
          const installedPluginIds = new Set(plugins.map((plugin) => plugin.id));

          return currentPluginIds.filter((pluginId) =>
            installedPluginIds.has(pluginId),
          );
        });

        if (plugins[0]) {
          setActiveRunPluginId((currentPluginId) =>
            plugins.some((plugin) => plugin.id === currentPluginId)
              ? currentPluginId
              : plugins[0].id,
          );
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
        setAvailablePlugins([]);
      })
      .finally(() => {
        if (isCurrent) {
          setIsPluginsLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [pluginRunDataSource]);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <Button
          size="xs"
          className="h-7 px-2 text-xs"
          disabled={!activeCase}
          onClick={() => setIsDataSourceWizardOpen(true)}
        >
          <Plus className="size-3.5" aria-hidden="true" />
          Add Data Source
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
          {selectedFileView && (
            <>
              <Separator orientation="vertical" className="h-4" />
              <span>View: {selectedFileView.name}</span>
            </>
          )}
        </div>
      </section>
      <DataSourceWizardDialog
        activeCase={activeCase}
        open={isDataSourceWizardOpen}
        onOpenChange={setIsDataSourceWizardOpen}
      />

      {(error || dataSourceError || entriesError || previewError) && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">
            {error ?? dataSourceError ?? entriesError ?? previewError}
          </span>
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
            rootNodes={treeRootNodes}
            selectedDirectory={selectedDirectory}
            selectedFileView={selectedFileView}
            fileViewCounts={fileViewCounts}
            fileViewDataSources={dataSources.map((dataSource) => ({
              id: dataSource.id,
              name: dataSource.name,
            }))}
            selectedFileViewDataSourceId={fileViewDataSourceId}
            tagSummaries={fileTagSummaries}
            onSelectFileViewDataSource={setFileViewDataSourceId}
            onSelectNode={selectTreeNode}
            onSelectFileView={(view) => {
              selectFileView(view);
            }}
            onRemoveDataSource={(node) => {
              void handleRemoveDataSource(node);
            }}
            onRunDataSourcePlugins={openRunPluginsDialog}
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
                isLoading={isEntriesLoading || isFileViewPageLoading}
                selectedEntry={selectedEntry}
                title={selectedFileView?.name}
                statusLabel={
                  selectedFileView
                    ? "Autopsy file view"
                    : "Directory-only acquisition"
                }
                emptyLabel={
                  selectedFileView
                    ? "No files match this view"
                    : "No files in this directory"
                }
                canGoBack={directoryHistory.length > 0}
                pageInfo={selectedFileViewPageInfo}
                isPageLoading={isFileViewPageLoading}
                onGoBack={goBackDirectory}
                onNextPage={loadNextFileViewPage}
                onOpenFolder={openFolderEntry}
                onPreviousPage={loadPreviousFileViewPage}
                onRemoveTag={(entry, tagName) => {
                  void handleRemoveFileTag(entry, tagName);
                }}
                onSelectEntry={setSelectedEntry}
                onSetTag={(input) => {
                  void handleSetFileTag(input);
                }}
                tagsByPath={fileTagsByPath}
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
                filePreview={filePreview}
                isLoading={isPreviewLoading}
                onActiveTabChange={setActivePreviewTab}
                selectedEntry={selectedEntry}
              />
            </ResizablePanel>
          </ResizablePanelGroup>
        </ResizablePanel>
      </ResizablePanelGroup>

      <Dialog
        open={Boolean(pluginRunDataSource)}
        onOpenChange={(isOpen) => {
          if (!isOpen) {
            setPluginRunDataSource(null);
            setPluginFilter("");
            setPluginTargetFilter("all");
          }
        }}
      >
        <DialogContent className="w-[calc(100vw-2rem)] max-w-7xl rounded-sm p-0 sm:max-w-7xl">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">Run Datasource Plugins</DialogTitle>
            <DialogDescription className="text-xs">
              Select Python plugins to run against{" "}
              {pluginRunDataSource?.name ?? "this datasource"}.
            </DialogDescription>
          </DialogHeader>

          <div className="grid h-[min(40rem,calc(100vh-10rem))] min-h-[30rem] grid-cols-[minmax(22rem,1fr)_minmax(20rem,0.8fr)] gap-2 p-2">
            <div className="flex min-h-0 min-w-0 flex-col gap-2">
              <div className="rounded-sm border">
                <div className="flex h-8 items-center justify-between border-b px-2">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Plugins
                  </div>
                  <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
                    {selectedRunPluginIds.length} selected
                  </Badge>
                </div>
                <div className="grid grid-cols-[minmax(0,1fr)_auto_auto] gap-1 p-2">
                  <Input
                    className="h-8 rounded-sm text-xs"
                    value={pluginFilter}
                    placeholder="Filter by name, type, or id"
                    onChange={(event) => setPluginFilter(event.target.value)}
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="xs"
                    className="h-7 rounded-sm px-2 text-xs"
                    disabled={visiblePlugins.length === 0}
                    onClick={() =>
                      setSelectedRunPluginIds(
                        visiblePlugins.map((plugin) => plugin.id),
                      )
                    }
                  >
                    Select All
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="xs"
                    className="h-7 rounded-sm px-2 text-xs"
                    disabled={selectedRunPluginIds.length === 0}
                    onClick={() => setSelectedRunPluginIds([])}
                  >
                    Unselect All
                  </Button>
                  <div className="col-span-3 flex flex-wrap gap-1">
                    {pluginTargetFilters.map((targetFilter) => {
                      const isSelected =
                        pluginTargetFilter === targetFilter.target;

                      return (
                        <Button
                          key={targetFilter.target}
                          type="button"
                          variant={isSelected ? "secondary" : "outline"}
                          size="xs"
                          className="h-7 rounded-sm px-2 text-[11px]"
                          onClick={() => setPluginTargetFilter(targetFilter.target)}
                        >
                          {targetFilter.label}
                        </Button>
                      );
                    })}
                  </div>
                </div>
              </div>
              <div className="min-h-0 flex-1 overflow-auto rounded-sm border">
                {visiblePlugins.length > 0 ? (
                  <div className="divide-y">
                    {visiblePlugins.map((plugin) => {
                      const isSelected = selectedRunPluginIds.includes(
                        plugin.id,
                      );
                      const isActive = activeRunPlugin?.id === plugin.id;

                      return (
                        <label
                          key={plugin.id}
                          className={cn(
                            "grid cursor-pointer grid-cols-[1rem_minmax(0,1fr)] items-start gap-2 px-2 py-2 text-xs hover:bg-accent",
                            isActive && "bg-accent",
                          )}
                          onClick={() => setActiveRunPluginId(plugin.id)}
                        >
                          <Checkbox
                            checked={isSelected}
                            onCheckedChange={(checked) => {
                              toggleRunPlugin(plugin, checked === true);
                            }}
                          />
                          <span className="min-w-0 space-y-1">
                            <span className="block truncate text-xs font-medium leading-none">
                              {plugin.name}
                            </span>
                            <span className="grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-center gap-1 text-[11px] leading-tight">
                              <Badge
                                variant="secondary"
                                className="h-4 shrink-0 rounded-sm px-1 text-[10px]"
                              >
                                {plugin.type}
                              </Badge>
                              <span className="block truncate text-muted-foreground">
                                {plugin.description}
                              </span>
                            </span>
                          </span>
                        </label>
                      );
                    })}
                  </div>
                ) : (
                  <div className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
                    {isPluginsLoading ? "Loading plugins" : "No plugins found."}
                  </div>
                )}
              </div>
            </div>

            <div className="flex min-h-0 min-w-0 flex-col rounded-sm border">
              <div className="flex h-8 items-center border-b px-2">
                <div className="text-xs font-medium uppercase text-muted-foreground">
                  Options
                </div>
              </div>
              {activeRunPlugin ? (
                <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto p-2 text-xs">
                  <div className="min-w-0">
                    <div className="truncate font-medium">
                      {activeRunPlugin.name}
                    </div>
                    <div className="mt-1 flex items-center gap-1">
                      <Badge
                        variant="secondary"
                        className="h-4 rounded-sm px-1 text-[10px]"
                      >
                        {getPluginTargetLabel(activeRunPlugin.target)}
                      </Badge>
                      <span className="truncate text-[11px] text-muted-foreground">
                        {activeRunPlugin.id}
                      </span>
                    </div>
                  </div>
                  <p className="text-[11px] text-muted-foreground">
                    {activeRunPlugin.description}
                  </p>
                  <label className="flex items-center gap-2 rounded-sm border px-2 py-1.5">
                    <Checkbox
                      checked={selectedRunPluginIds.includes(activeRunPlugin.id)}
                      onCheckedChange={(checked) => {
                        toggleRunPlugin(activeRunPlugin, checked === true);
                      }}
                    />
                    <span>Run plugin</span>
                  </label>
                  <Separator />
                  <div className="grid grid-cols-[5rem_1fr] gap-x-2 gap-y-1">
                    <div className="text-muted-foreground">Datasource</div>
                    <div className="truncate">{pluginRunDataSource?.name}</div>
                    <div className="text-muted-foreground">Sources</div>
                    <div>{pluginRunDataSource?.paths.length ?? 0}</div>
                    <div className="text-muted-foreground">Mode</div>
                    <div>{activeRunPlugin.mode}</div>
                    <div className="text-muted-foreground">Type</div>
                    <div className="truncate">{activeRunPlugin.type}</div>
                    <div className="text-muted-foreground">Target</div>
                    <div>{getPluginTargetLabel(activeRunPlugin.target)}</div>
                  </div>
                  {(activeRunPlugin.options ?? []).length > 0 ? (
                    <div className="mt-auto grid gap-2 rounded-sm border p-2">
                      {activeRunPlugin.options?.map((option) => (
                        <label key={option.id} className="grid gap-1">
                          <span className="font-medium">{option.label}</span>
                          <select
                            className="h-7 rounded-sm border bg-background px-2 text-xs"
                            value={
                              pluginOptionValues[activeRunPlugin.id]?.[
                                option.id
                              ] ?? option.defaultValue
                            }
                            onChange={(event) => {
                              const value = event.target.value;
                              setPluginOptionValues((current) => ({
                                ...current,
                                [activeRunPlugin.id]: {
                                  ...current[activeRunPlugin.id],
                                  [option.id]: value,
                                },
                              }));
                            }}
                          >
                            {option.choices.map((choice) => (
                              <option key={choice.value} value={choice.value}>
                                {choice.label}
                              </option>
                            ))}
                          </select>
                          {option.description && (
                            <span className="text-[10px] text-muted-foreground">
                              {option.description}
                            </span>
                          )}
                        </label>
                      ))}
                    </div>
                  ) : (
                    <div className="mt-auto rounded-sm border border-dashed px-2 py-2 text-[11px] text-muted-foreground">
                      This plugin does not expose configurable options yet.
                    </div>
                  )}
                </div>
              ) : (
                <div className="grid flex-1 place-items-center px-3 text-center text-xs text-muted-foreground">
                  Select a plugin to view options.
                </div>
              )}
            </div>
          </div>

          <DialogFooter className="border-t p-2">
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              onClick={() => setPluginRunDataSource(null)}
            >
              Close
            </Button>
            <Button
              type="button"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              disabled={isRunningPlugins || selectedRunPluginIds.length === 0}
              onClick={() => {
                void runSelectedPlugins();
              }}
            >
              <Play className="size-3.5" aria-hidden="true" />
              {isRunningPlugins ? "Running" : "Run Plugins"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <footer className="flex h-6 shrink-0 items-center gap-3 border-t px-2 text-[11px] text-muted-foreground">
        <span>
          {isLoading || isDataSourcesLoading || isEntriesLoading
            ? "Loading"
            : "Ready"}
        </span>
        <span>Evidence: {listing?.rootName ?? "none"}</span>
        <span>Datasources: {dataSourceTreeNodes.length}</span>
        <span>
          {selectedFileView
            ? `View: ${selectedFileView.name}`
            : `Folder: ${selectedDirectory?.name ?? "none"}`}
        </span>
        <span>Files indexed: {visibleEntries.length}</span>
        <span>Plugin jobs: 0 running</span>
      </footer>
    </div>
  );
}

async function buildDataSourceTreeNode(
  dataSource: DataSourceRecord,
): Promise<EvidenceTreeNode> {
  const entries = await invoke<EvidenceDirectoryEntry[]>("describe_paths", {
    paths: dataSource.paths,
  });
  const pathNodes = await Promise.all(
    entries.map((entry) => buildDataSourcePathTreeNode(dataSource, entry)),
  );
  const children = entries.flatMap((entry, index) => {
    const node = pathNodes[index];

    if (!isArchiveExtractorOutputPath(dataSource, entry.path)) {
      return [node];
    }

    return (node.children ?? []).map((archiveNode) => ({
      ...archiveNode,
      name: getArchiveTreeNodeName(archiveNode.name),
    }));
  });

  return {
    id: `datasource:${dataSource.id}`,
    name: dataSource.name,
    path: dataSource.path,
    kind: "datasource",
    files: entries.length,
    children,
  };
}

async function buildDataSourcePathTreeNode(
  dataSource: DataSourceRecord,
  entry: EvidenceDirectoryEntry,
): Promise<EvidenceTreeNode> {
  if (entry.kind === "directory") {
    try {
      const listing = await invoke<EvidenceDirectoryListing>("list_directory", {
        path: entry.path,
      });
      return prefixTreeNodeId(listing.tree, `datasource:${dataSource.id}`);
    } catch {
      return directoryEntryToTreeNode(dataSource, entry);
    }
  }

  return directoryEntryToTreeNode(dataSource, entry);
}

function directoryEntryToTreeNode(
  dataSource: DataSourceRecord,
  entry: EvidenceDirectoryEntry,
): EvidenceTreeNode {
  return {
    id: `datasource:${dataSource.id}:${entry.path}`,
    name: entry.name,
    path: entry.path,
    kind: entry.kind,
    files: entry.kind === "directory" ? (entry.childCount ?? 0) : 0,
    size: entry.size,
    modifiedMs: entry.modifiedMs,
    childCount: entry.childCount,
  };
}

function getArchiveTreeNodeName(name: string) {
  return name.replace(/^\d{4}-/, "");
}

function isArchiveExtractorOutputPath(
  dataSource: DataSourceRecord,
  path: string,
) {
  const normalizedPath = path
    .replace(/\\/g, "/")
    .replace(/\/+$/g, "")
    .toLowerCase();
  const normalizedDataSourceId = dataSource.id.toLowerCase();

  return normalizedPath.endsWith(
    `/artifacts/extracted/${normalizedDataSourceId}`,
  );
}

function prefixTreeNodeId(
  node: EvidenceTreeNode,
  idPrefix: string,
): EvidenceTreeNode {
  return {
    ...node,
    id: `${idPrefix}:${node.path}`,
    children: node.children?.map((childNode) =>
      prefixTreeNodeId(childNode, idPrefix),
    ),
  };
}

function treeNodeToDirectoryEntry(
  node: EvidenceTreeNode,
): EvidenceDirectoryEntry {
  return {
    id: node.path,
    name: node.name,
    path: node.path,
    kind: node.kind === "datasource" ? "directory" : node.kind,
    size: node.size,
    modifiedMs: node.modifiedMs,
    childCount: node.childCount ?? node.files,
  };
}

function fileViewToDirectoryEntry(view: FileViewSelection): EvidenceDirectoryEntry {
  return {
    id: `file-view:${view.id}`,
    name: view.name,
    path: view.description,
    kind: "directory",
    childCount: view.count,
  };
}

function applyFileViewCounts(
  view: FileViewSelection,
  counts: Record<string, number>,
): FileViewSelection {
  const childViews = view.childViews?.map((childView) =>
    applyFileViewCounts(childView, counts),
  );
  const count = childViews?.length
    ? childViews.reduce((total, childView) => total + childView.count, 0)
    : counts[view.id] ?? view.count;

  return {
    ...view,
    count,
    childViews,
  };
}
