import { useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Copy,
  Folder,
  FolderInput,
  FolderOpen,
  FolderPlus,
  Plus,
  Play,
  RefreshCw,
  Trash2,
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
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { buildCultivatorApiReferenceForLlms } from "@/features/artifacts/llmApiReference";
import { useCases } from "@/features/cases/case-provider";
import {
  listDataSources,
  notifyDataSourcesChanged,
} from "@/features/datasources/dataSourceRepository";
import type { DataSourceRecord } from "@/features/datasources/types";
import {
  getStoredCreatePluginMode,
  storeCreatePluginMode,
} from "@/features/plugins/createPluginPreferences";
import {
  isSafePluginFolderName,
  isSafePluginOrganizationPath,
} from "@/features/plugins/pluginManifest";
import {
  cancelDatasourcePluginRun,
  createPythonPlugin,
  createPythonPluginFolder,
  deletePythonPlugin,
  listPluginJobs,
  listPluginLogs,
  listPythonPluginFolders,
  listPythonPlugins,
  movePythonPlugin,
  openPythonPluginFolder,
  openPythonPluginFolderInVscode,
  pluginRunUpdatedDatasourcePaths,
  runDatasourcePlugins,
} from "@/features/plugins/pluginRepository";
import { createPluginRunId } from "@/features/plugins/pluginToasts";
import type {
  PluginJobRecord,
  PluginLogRecord,
  PythonPlugin,
} from "@/features/plugins/types";
import { cn } from "@/lib/utils";

type LoadState = {
  error: string | null;
  isLoading: boolean;
};

type ActivePluginRun = {
  datasourceId: string;
  pluginIds: string[];
  runId: string;
  status: "running" | "cancelling";
};

type CreatePluginMode = "manual" | "automatic";
type CreatePluginTarget =
  | "ios"
  | "android"
  | "windows"
  | "macos"
  | "infotainment"
  | "other";
type CreatePluginRunMode = "each_file" | "path_glob" | "path_regex";

const createPluginTargets: Array<{ value: CreatePluginTarget; label: string }> = [
  { value: "infotainment", label: "Infotainment" },
  { value: "android", label: "Android" },
  { value: "ios", label: "iOS" },
  { value: "windows", label: "Windows" },
  { value: "macos", label: "macOS" },
  { value: "other", label: "Other" },
];

const createPluginModes: Array<{ value: CreatePluginRunMode; label: string }> = [
  { value: "each_file", label: "Each file" },
  { value: "path_glob", label: "Path glob" },
  { value: "path_regex", label: "Path regex" },
];

function getErrorMessage(caughtError: unknown) {
  return caughtError instanceof Error ? caughtError.message : String(caughtError);
}

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

function formatJobTime(value: string | null) {
  if (!value) {
    return "-";
  }

  const numericValue = Number(value);
  const date = Number.isFinite(numericValue)
    ? new Date(numericValue)
    : new Date(value);

  if (Number.isNaN(date.getTime())) {
    return value;
  }

  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(date);
}

function getStatusBadgeClassName(status: PluginJobRecord["status"]) {
  if (status === "complete") {
    return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  }

  if (status === "failed") {
    return "bg-destructive/10 text-destructive";
  }

  return "";
}

function getLatestDatasourceJob(
  jobs: PluginJobRecord[],
  datasourceId: string,
) {
  return jobs.find((job) => job.datasourceId === datasourceId) ?? null;
}

function getLatestPluginJob(
  jobs: PluginJobRecord[],
  datasourceId: string,
  pluginId: string,
) {
  return (
    jobs.find(
      (job) => job.datasourceId === datasourceId && job.pluginId === pluginId,
    ) ?? null
  );
}

function getPluginLabel(pluginMap: Map<string, PythonPlugin>, pluginId: string) {
  return pluginMap.get(pluginId)?.name ?? pluginId;
}

function mergePluginJobs(
  currentJobs: PluginJobRecord[],
  nextJobs: PluginJobRecord[],
) {
  const jobsById = new Map(currentJobs.map((job) => [job.id, job]));

  for (const job of nextJobs) {
    jobsById.set(job.id, job);
  }

  return Array.from(jobsById.values()).sort((firstJob, secondJob) =>
    secondJob.startedAt.localeCompare(firstJob.startedAt),
  );
}

export function PluginsPage() {
  const { activeCase } = useCases();
  const [datasources, setDatasources] = useState<DataSourceRecord[]>([]);
  const [plugins, setPlugins] = useState<PythonPlugin[]>([]);
  const [pluginFolders, setPluginFolders] = useState<string[]>([]);
  const [jobs, setJobs] = useState<PluginJobRecord[]>([]);
  const [logs, setLogs] = useState<PluginLogRecord[]>([]);
  const [activeRuns, setActiveRuns] = useState<ActivePluginRun[]>([]);
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [isCreateFolderDialogOpen, setIsCreateFolderDialogOpen] =
    useState(false);
  const [createPluginMode, setCreatePluginMode] = useState<CreatePluginMode>(
    getStoredCreatePluginMode,
  );
  const [newPluginId, setNewPluginId] = useState("");
  const [newPluginName, setNewPluginName] = useState("");
  const [newPluginOrganizationFolder, setNewPluginOrganizationFolder] =
    useState("");
  const [newPluginAuthor, setNewPluginAuthor] = useState("");
  const [newPluginVersion, setNewPluginVersion] = useState("1.0.0");
  const [newPluginDescription, setNewPluginDescription] = useState("");
  const [newPluginType, setNewPluginType] = useState("other");
  const [newPluginTarget, setNewPluginTarget] =
    useState<CreatePluginTarget>("infotainment");
  const [newPluginRunMode, setNewPluginRunMode] =
    useState<CreatePluginRunMode>("each_file");
  const [newPluginPathGlob, setNewPluginPathGlob] = useState("");
  const [newPluginPathRegex, setNewPluginPathRegex] = useState("");
  const [newPluginEntry, setNewPluginEntry] = useState("plugin.py");
  const [newPluginFunction, setNewPluginFunction] = useState("run");
  const [newPluginTomlDetails, setNewPluginTomlDetails] = useState("");
  const [creatingPlugin, setCreatingPlugin] = useState(false);
  const [newOrganizationFolder, setNewOrganizationFolder] = useState("");
  const [creatingOrganizationFolder, setCreatingOrganizationFolder] =
    useState(false);
  const [deletingPluginId, setDeletingPluginId] = useState<string | null>(null);
  const [movingPlugin, setMovingPlugin] = useState<PythonPlugin | null>(null);
  const [isMovingPlugin, setIsMovingPlugin] = useState(false);
  const [isBulkMoveDialogOpen, setIsBulkMoveDialogOpen] = useState(false);
  const [bulkMoveSource, setBulkMoveSource] = useState("");
  const [bulkMovePluginIds, setBulkMovePluginIds] = useState<string[]>([]);
  const [bulkMoveDestination, setBulkMoveDestination] = useState("");
  const [bulkMoveProgress, setBulkMoveProgress] = useState<{
    completed: number;
    total: number;
  } | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied">("idle");
  const [loadState, setLoadState] = useState<LoadState>({
    error: null,
    isLoading: false,
  });
  const pluginMap = useMemo(() => {
    return new Map(plugins.map((plugin) => [plugin.id, plugin]));
  }, [plugins]);
  const datasourceMap = useMemo(() => {
    return new Map(datasources.map((datasource) => [datasource.id, datasource]));
  }, [datasources]);
  const movablePlugins = useMemo(
    () => plugins.filter((plugin) => !plugin.entry.startsWith("builtin:")),
    [plugins],
  );
  const visibleBulkMovePlugins = useMemo(
    () =>
      movablePlugins.filter(
        (plugin) => (plugin.organizationFolder ?? "") === bulkMoveSource,
      ),
    [bulkMoveSource, movablePlugins],
  );
  const runningPluginRows = useMemo(() => {
    const activeRows = activeRuns.flatMap((run) =>
      run.pluginIds.map((pluginId) => {
        const job = getLatestPluginJob(jobs, run.datasourceId, pluginId);

        return {
          datasource: datasourceMap.get(run.datasourceId) ?? null,
          job,
          pluginId,
          run,
        };
      }),
    );
    const activeJobIds = new Set(
      activeRows
        .map((row) => row.job?.id)
        .filter((jobId): jobId is string => Boolean(jobId)),
    );
    const backendRows = jobs
      .filter((job) => job.status === "running" && !activeJobIds.has(job.id))
      .map((job) => ({
        datasource: datasourceMap.get(job.datasourceId) ?? null,
        job,
        pluginId: job.pluginId,
        run: activeRuns.find(
          (activeRun) =>
            activeRun.datasourceId === job.datasourceId &&
            activeRun.pluginIds.includes(job.pluginId),
        ) ?? null,
      }));

    return [...activeRows, ...backendRows];
  }, [activeRuns, datasourceMap, jobs]);

  async function refreshPluginsPage(options: { showLoading?: boolean } = {}) {
    const showLoading = options.showLoading ?? true;

    if (!activeCase) {
      setDatasources([]);
      setPlugins([]);
      setPluginFolders([]);
      setJobs([]);
      setLogs([]);
      setLoadState({ error: null, isLoading: false });
      return;
    }

    if (showLoading) {
      setLoadState({ error: null, isLoading: true });
    }

    try {
      const [nextDatasources, nextPlugins, nextPluginFolders, nextJobs, nextLogs] =
        await Promise.all([
          listDataSources(activeCase.databasePath, activeCase.id),
          listPythonPlugins(),
          listPythonPluginFolders(),
          listPluginJobs(activeCase.databasePath),
          listPluginLogs(activeCase.databasePath),
        ]);

      setDatasources(nextDatasources);
      setPlugins(nextPlugins);
      setPluginFolders(nextPluginFolders);
      setJobs(nextJobs);
      setLogs(nextLogs);
      setLoadState({ error: null, isLoading: false });
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    }
  }

  async function runPlugins(datasource: DataSourceRecord) {
    if (!activeCase) {
      return;
    }

    setLoadState((currentState) => ({ ...currentState, error: null }));
    const runId = createPluginRunId();
    const activeRun: ActivePluginRun = {
      datasourceId: datasource.id,
      pluginIds: datasource.pluginIds,
      runId,
      status: "running",
    };

    setActiveRuns((currentRuns) => [...currentRuns, activeRun]);

    try {
      const summary = await runDatasourcePlugins({
        caseDatabasePath: activeCase.databasePath,
        caseFolderPath: activeCase.folderPath,
        datasourceId: datasource.id,
        runId,
      });
      setJobs((currentJobs) => mergePluginJobs(currentJobs, summary.jobs));
      if (pluginRunUpdatedDatasourcePaths(summary)) {
        notifyDataSourcesChanged(activeCase.id);
      }
      await refreshPluginsPage();
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    } finally {
      setActiveRuns((currentRuns) =>
        currentRuns.filter((currentRun) => currentRun.runId !== runId),
      );
    }
  }

  async function cancelPluginRun(runId: string) {
    setActiveRuns((currentRuns) =>
      currentRuns.map((run) =>
        run.runId === runId ? { ...run, status: "cancelling" } : run,
      ),
    );

    try {
      await cancelDatasourcePluginRun(runId);
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
      setActiveRuns((currentRuns) =>
        currentRuns.map((run) =>
          run.runId === runId ? { ...run, status: "running" } : run,
        ),
      );
    }
  }

  async function createPlugin() {
    const pluginName = newPluginName.trim();
    const pluginId = newPluginId.trim();
    const pluginType = newPluginType.trim();
    const pluginEntry = newPluginEntry.trim();
    const pluginFunction = newPluginFunction.trim();
    const pluginTomlDetails = newPluginTomlDetails.trim();
    const organizationFolder = newPluginOrganizationFolder.trim();

    if (!isSafePluginOrganizationPath(organizationFolder)) {
      setLoadState({
        error:
          "Organization folders must be relative paths using letters, numbers, spaces, '.', '_', and '-'.",
        isLoading: false,
      });
      return;
    }

    if (createPluginMode === "manual") {
      if (!pluginId || !pluginName || !pluginType || !pluginEntry || !pluginFunction) {
        setLoadState({
          error: "Plugin id, name, type, entry, and function are required.",
          isLoading: false,
        });
        return;
      }

      if (newPluginRunMode === "path_glob" && !newPluginPathGlob.trim()) {
        setLoadState({
          error: "Path glob mode requires at least one path glob.",
          isLoading: false,
        });
        return;
      }

      if (newPluginRunMode === "path_regex" && !newPluginPathRegex.trim()) {
        setLoadState({
          error: "Path regex mode requires a path regex.",
          isLoading: false,
        });
        return;
      }
    } else {
      if (!pluginName) {
        setLoadState({ error: "Plugin folder name is required.", isLoading: false });
        return;
      }

      if (!isSafePluginFolderName(pluginName)) {
        setLoadState({
          error:
            "Plugin folder name may only contain letters, numbers, '.', '_', and '-'.",
          isLoading: false,
        });
        return;
      }

      if (!pluginTomlDetails) {
        setLoadState({
          error: "Plugin TOML details are required.",
          isLoading: false,
        });
        return;
      }
    }

    setCreatingPlugin(true);
    setLoadState((currentState) => ({ ...currentState, error: null }));

    try {
      await createPythonPlugin(
        createPluginMode === "manual"
          ? {
              organizationFolder: organizationFolder || undefined,
              manifest: {
                id: pluginId,
                name: pluginName,
                author: newPluginAuthor.trim() || "Unknown",
                version: newPluginVersion.trim() || "1.0.0",
                description: newPluginDescription.trim(),
                type: pluginType,
                target: newPluginTarget,
                mode: newPluginRunMode,
                pathGlob: newPluginPathGlob
                  .split(/\r?\n|,/)
                  .map((pathGlob) => pathGlob.trim())
                  .filter(Boolean),
                pathRegex: newPluginPathRegex.trim() || undefined,
                entry: pluginEntry,
                function: pluginFunction,
              },
            }
          : {
              folderName: pluginName,
              organizationFolder: organizationFolder || undefined,
              manifestToml: pluginTomlDetails,
            },
      );
      await openPythonPluginFolderInVscode();
      resetCreatePluginForm();
      setIsCreateDialogOpen(false);
      await refreshPluginsPage();
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    } finally {
      setCreatingPlugin(false);
    }
  }

  function resetCreatePluginForm() {
    setNewPluginId("");
    setNewPluginName("");
    setNewPluginOrganizationFolder("");
    setNewPluginAuthor("");
    setNewPluginVersion("1.0.0");
    setNewPluginDescription("");
    setNewPluginType("other");
    setNewPluginTarget("infotainment");
    setNewPluginRunMode("each_file");
    setNewPluginPathGlob("");
    setNewPluginPathRegex("");
    setNewPluginEntry("plugin.py");
    setNewPluginFunction("run");
    setNewPluginTomlDetails("");
  }

  async function createOrganizationFolder() {
    const folder = newOrganizationFolder.trim();
    if (!folder || !isSafePluginOrganizationPath(folder)) {
      setLoadState({
        error:
          "Enter a relative folder path using letters, numbers, spaces, '.', '_', and '-'.",
        isLoading: false,
      });
      return;
    }

    setCreatingOrganizationFolder(true);
    setLoadState((currentState) => ({ ...currentState, error: null }));
    try {
      await createPythonPluginFolder(folder);
      setNewOrganizationFolder("");
      setIsCreateFolderDialogOpen(false);
      await refreshPluginsPage({ showLoading: false });
    } catch (caughtError) {
      setLoadState({ error: getErrorMessage(caughtError), isLoading: false });
    } finally {
      setCreatingOrganizationFolder(false);
    }
  }

  function handleCreatePluginModeChange(value: string) {
    const nextMode = value === "automatic" ? "automatic" : "manual";

    setCreatePluginMode(nextMode);
    storeCreatePluginMode(nextMode);
  }

  async function deletePlugin(plugin: PythonPlugin) {
    const shouldDelete = window.confirm(
      `Delete Python plugin "${plugin.name}"?\n\nThis removes the plugin folder from disk.`,
    );

    if (!shouldDelete) {
      return;
    }

    setDeletingPluginId(plugin.id);
    setLoadState((currentState) => ({ ...currentState, error: null }));

    try {
      await deletePythonPlugin(plugin.id);
      await refreshPluginsPage();
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    } finally {
      setDeletingPluginId(null);
    }
  }

  async function movePlugin(folder: string) {
    if (!movingPlugin) {
      return;
    }

    setIsMovingPlugin(true);
    setLoadState((currentState) => ({ ...currentState, error: null }));
    try {
      await movePythonPlugin(movingPlugin.id, folder);
      setMovingPlugin(null);
      await refreshPluginsPage();
    } catch (caughtError) {
      setLoadState({ error: getErrorMessage(caughtError), isLoading: false });
    } finally {
      setIsMovingPlugin(false);
    }
  }

  function toggleBulkMovePlugin(pluginId: string, selected: boolean) {
    setBulkMovePluginIds((currentPluginIds) =>
      selected
        ? currentPluginIds.includes(pluginId)
          ? currentPluginIds
          : [...currentPluginIds, pluginId]
        : currentPluginIds.filter((currentPluginId) => currentPluginId !== pluginId),
    );
  }

  async function bulkMovePlugins() {
    if (bulkMovePluginIds.length === 0) {
      setLoadState({
        error: "Select at least one Python plugin to move.",
        isLoading: false,
      });
      return;
    }

    const failures: Array<{ id: string; error: string }> = [];
    setBulkMoveProgress({ completed: 0, total: bulkMovePluginIds.length });
    setLoadState((currentState) => ({ ...currentState, error: null }));

    for (const [index, pluginId] of bulkMovePluginIds.entries()) {
      try {
        await movePythonPlugin(pluginId, bulkMoveDestination);
      } catch (caughtError) {
        failures.push({ id: pluginId, error: getErrorMessage(caughtError) });
      }
      setBulkMoveProgress({
        completed: index + 1,
        total: bulkMovePluginIds.length,
      });
    }

    await refreshPluginsPage({ showLoading: false });
    setBulkMoveProgress(null);

    if (failures.length === 0) {
      setBulkMovePluginIds([]);
      setBulkMoveDestination("");
      setIsBulkMoveDialogOpen(false);
      return;
    }

    setBulkMovePluginIds(failures.map((failure) => failure.id));
    setLoadState({
      error: `${failures.length} plugin${failures.length === 1 ? "" : "s"} could not be moved: ${failures[0].error}`,
      isLoading: false,
    });
  }

  async function openPluginFolder() {
    setLoadState((currentState) => ({ ...currentState, error: null }));

    try {
      await openPythonPluginFolder();
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    }
  }

  async function copyApiForLlms() {
    setLoadState((currentState) => ({ ...currentState, error: null }));

    try {
      await copyTextToClipboard(buildCultivatorApiReferenceForLlms());
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 2000);
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    }
  }

  useEffect(() => {
    void refreshPluginsPage();
  }, [activeCase?.id]);

  useEffect(() => {
    if (!activeCase) {
      return;
    }

    const intervalId = window.setInterval(() => {
      void refreshPluginsPage({ showLoading: false });
    }, 2000);

    return () => window.clearInterval(intervalId);
  }, [activeCase?.id]);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 overflow-x-auto border-b px-2">
        <div className="text-xs font-medium uppercase text-muted-foreground">
          Python Plugins
        </div>
        <Separator orientation="vertical" className="h-5" />
        <Button
          type="button"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          onClick={() => setIsCreateDialogOpen(true)}
        >
          <Plus className="size-3.5" aria-hidden="true" />
          Add Plugin
        </Button>
        <Button
          type="button"
          variant="outline"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          onClick={() => setIsCreateFolderDialogOpen(true)}
        >
          <FolderPlus className="size-3.5" aria-hidden="true" />
          New Folder
        </Button>
        <Button
          type="button"
          variant="outline"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          disabled={movablePlugins.length === 0}
          onClick={() => {
            setBulkMovePluginIds([]);
            setBulkMoveDestination("");
            setIsBulkMoveDialogOpen(true);
          }}
        >
          <FolderInput className="size-3.5" aria-hidden="true" />
          Move Plugins
        </Button>
        <Button
          type="button"
          variant="outline"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          onClick={() => {
            void openPluginFolder();
          }}
        >
          <FolderOpen className="size-3.5" aria-hidden="true" />
          Open Folder
        </Button>
        <Button
          type="button"
          variant="outline"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          onClick={() => {
            void copyApiForLlms();
          }}
        >
          <Copy className="size-3.5" aria-hidden="true" />
          {copyState === "copied" ? "Copied API" : "Copy API for LLMs"}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          disabled={loadState.isLoading}
          onClick={() => {
            void refreshPluginsPage();
          }}
        >
          <RefreshCw className="size-3.5" aria-hidden="true" />
          Refresh
        </Button>
        <div className="ml-auto flex items-center gap-1 text-[11px] text-muted-foreground">
          <span>{plugins.length} installed plugins</span>
          <Separator orientation="vertical" className="h-4" />
          <span>{datasources.length} datasources</span>
        </div>
      </section>

      {loadState.error && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{loadState.error}</span>
        </section>
      )}

      <ResizablePanelGroup
        orientation="horizontal"
        className="min-h-0 min-w-0 flex-1"
      >
        <ResizablePanel
          defaultSize="50%"
          minSize="24%"
          className="min-h-0 min-w-0 overflow-hidden"
        >
          <ResizablePanelGroup
            orientation="vertical"
            className="h-full min-h-0 min-w-0"
          >
            <ResizablePanel
              defaultSize="50%"
              minSize="24%"
              className="min-h-0 min-w-0 overflow-hidden"
            >
              <section className="flex h-full min-h-0 min-w-0 flex-col border-r border-b">
                <div className="flex h-8 shrink-0 items-center justify-between border-b px-2">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Installed Plugins
                  </div>
                  <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
                    {plugins.length}
                  </Badge>
                </div>

                <Table
                  containerClassName="min-h-0 flex-1 overflow-auto"
                  className="min-w-[960px] table-fixed text-xs"
                >
                  <TableHeader className="sticky top-0 z-10 bg-muted">
                    <TableRow className="hover:bg-muted">
                      <TableHead className="h-7 w-[190px] px-2">Plugin</TableHead>
                      <TableHead className="h-7 w-[120px] px-2">Folder</TableHead>
                      <TableHead className="h-7 w-[130px] px-2">Author</TableHead>
                      <TableHead className="h-7 w-[70px] px-2">Version</TableHead>
                      <TableHead className="h-7 w-[110px] px-2">Target</TableHead>
                      <TableHead className="h-7 w-[110px] px-2">Mode</TableHead>
                      <TableHead className="h-7 w-[220px] px-2">Description</TableHead>
                      <TableHead className="h-7 w-[110px] px-2">Entry</TableHead>
                      <TableHead className="h-7 w-[150px] px-2">Action</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {plugins.map((plugin) => {
                      const isDeleting = deletingPluginId === plugin.id;
                      const isBuiltIn = plugin.entry.startsWith("builtin:");

                      return (
                        <TableRow key={plugin.id} className="h-8">
                          <TableCell className="h-8 px-2 py-0">
                            <div className="min-w-0">
                              <div className="truncate font-medium">{plugin.name}</div>
                              <div className="truncate font-mono text-[10px] text-muted-foreground">
                                {plugin.id}
                              </div>
                            </div>
                          </TableCell>
                          <TableCell className="h-8 truncate px-2 py-0 font-mono text-[11px] text-muted-foreground">
                            {isBuiltIn
                              ? "Built-in"
                              : plugin.organizationFolder || "Root"}
                          </TableCell>
                          <TableCell className="h-8 truncate px-2 py-0 text-muted-foreground">
                            {plugin.author || "Unknown"}
                          </TableCell>
                          <TableCell className="h-8 truncate px-2 py-0 font-mono text-[11px] text-muted-foreground">
                            {plugin.version || "0.0.0"}
                          </TableCell>
                          <TableCell className="h-8 px-2 py-0">
                            <Badge
                              variant="secondary"
                              className="h-5 rounded-sm px-1 text-[10px]"
                            >
                              {getPluginTargetLabel(plugin.target)}
                            </Badge>
                          </TableCell>
                          <TableCell className="h-8 px-2 py-0">
                            <Badge
                              variant="outline"
                              className="h-5 rounded-sm px-1 text-[10px]"
                            >
                              {plugin.mode}
                            </Badge>
                          </TableCell>
                          <TableCell className="h-8 truncate px-2 py-0 text-muted-foreground">
                            {plugin.description || "-"}
                          </TableCell>
                          <TableCell className="h-8 truncate px-2 py-0 font-mono text-[11px] text-muted-foreground">
                            {plugin.entry}:{plugin.function}
                          </TableCell>
                          <TableCell className="h-8 px-2 py-0">
                            <div className="flex items-center gap-1">
                              <Button
                                type="button"
                                variant="ghost"
                                size="xs"
                                className="h-7 rounded-sm px-2 text-xs"
                                disabled={isBuiltIn}
                                onClick={() => {
                                  setMovingPlugin(plugin);
                                }}
                              >
                                <FolderInput
                                  className="size-3.5"
                                  aria-hidden="true"
                                />
                                Move
                              </Button>
                              <Button
                                type="button"
                                variant="ghost"
                                size="xs"
                                className="h-7 rounded-sm px-2 text-xs text-destructive hover:text-destructive"
                                disabled={isDeleting || isBuiltIn}
                                onClick={() => {
                                  void deletePlugin(plugin);
                                }}
                              >
                                <Trash2
                                  className="size-3.5"
                                  aria-hidden="true"
                                />
                                {isBuiltIn
                                  ? "Built-in"
                                  : isDeleting
                                    ? "Deleting"
                                    : "Delete"}
                              </Button>
                            </div>
                          </TableCell>
                        </TableRow>
                      );
                    })}
                    {plugins.length === 0 && (
                      <TableRow>
                        <TableCell
                          colSpan={9}
                          className="h-16 text-center text-xs text-muted-foreground"
                        >
                          No Python plugins are installed.
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </section>
            </ResizablePanel>

            <ResizableHandle withHandle />

            <ResizablePanel
              defaultSize="50%"
              minSize="24%"
              className="min-h-0 min-w-0 overflow-hidden"
            >
              <section className="flex h-full min-h-0 min-w-0 flex-col border-r">
                <div className="flex h-8 shrink-0 items-center justify-between border-b px-2">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Datasource Jobs
                  </div>
                  <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
                    {loadState.isLoading ? "Loading" : "Ready"}
                  </Badge>
                </div>

                <Table
                  containerClassName="min-h-0 flex-1 overflow-auto"
                  className="min-w-[920px] table-fixed text-xs"
                >
                  <TableHeader className="sticky top-0 z-10 bg-muted">
                    <TableRow className="hover:bg-muted">
                      <TableHead className="h-7 w-[220px] px-2">Datasource</TableHead>
                      <TableHead className="h-7 w-[90px] px-2">Sources</TableHead>
                      <TableHead className="h-7 w-[260px] px-2">Selected Plugins</TableHead>
                      <TableHead className="h-7 w-[110px] px-2">Last Status</TableHead>
                      <TableHead className="h-7 w-[160px] px-2">Last Run</TableHead>
                      <TableHead className="h-7 w-[180px] px-2">Action</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {datasources.map((datasource) => {
                      const latestJob = getLatestDatasourceJob(jobs, datasource.id);
                      const activeRun = activeRuns.find(
                        (run) => run.datasourceId === datasource.id,
                      );
                      const isRunning = Boolean(activeRun);

                      return (
                        <TableRow key={datasource.id} className="h-8">
                          <TableCell className="h-8 truncate px-2 py-0 font-medium">
                            {datasource.name}
                          </TableCell>
                          <TableCell className="h-8 px-2 py-0">
                            {datasource.paths.length}
                          </TableCell>
                          <TableCell className="h-8 truncate px-2 py-0 text-muted-foreground">
                            {datasource.pluginIds.length > 0
                              ? datasource.pluginIds
                                  .map((pluginId) => getPluginLabel(pluginMap, pluginId))
                                  .join(", ")
                              : "None selected"}
                          </TableCell>
                          <TableCell className="h-8 px-2 py-0">
                            {latestJob ? (
                              <Badge
                                variant="secondary"
                                className={cn(
                                  "h-5 rounded-sm text-[11px]",
                                  getStatusBadgeClassName(latestJob.status),
                                )}
                              >
                                {latestJob.status}
                              </Badge>
                            ) : (
                              "-"
                            )}
                          </TableCell>
                          <TableCell className="h-8 px-2 py-0 text-muted-foreground">
                            {formatJobTime(latestJob?.startedAt ?? null)}
                          </TableCell>
                          <TableCell className="h-8 px-2 py-0">
                            {activeRun ? (
                              <Button
                                type="button"
                                variant="outline"
                                size="xs"
                                className="h-7 rounded-sm px-2 text-xs"
                                disabled={activeRun.status === "cancelling"}
                                onClick={() => {
                                  void cancelPluginRun(activeRun.runId);
                                }}
                              >
                                {activeRun.status === "cancelling"
                                  ? "Cancelling"
                                  : "Cancel"}
                              </Button>
                            ) : (
                              <Button
                                type="button"
                                size="xs"
                                className="h-7 rounded-sm px-2 text-xs"
                                disabled={
                                  !activeCase ||
                                  datasource.pluginIds.length === 0 ||
                                  isRunning
                                }
                                onClick={() => {
                                  void runPlugins(datasource);
                                }}
                              >
                                <Play className="size-3.5" aria-hidden="true" />
                                Run plugins
                              </Button>
                            )}
                          </TableCell>
                        </TableRow>
                      );
                    })}
                    {datasources.length === 0 && (
                      <TableRow>
                        <TableCell
                          colSpan={6}
                          className="h-20 text-center text-xs text-muted-foreground"
                        >
                          {activeCase
                            ? "No datasources have been added to this case."
                            : "Create or select a case before running plugins."}
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </section>
            </ResizablePanel>
          </ResizablePanelGroup>
        </ResizablePanel>

        <ResizableHandle withHandle />

        <ResizablePanel
          defaultSize="50%"
          minSize="24%"
          className="min-h-0 min-w-0 overflow-hidden"
        >
          <ResizablePanelGroup
            orientation="vertical"
            className="h-full min-h-0 min-w-0"
          >
            <ResizablePanel
              defaultSize="50%"
              minSize="24%"
              className="min-h-0 min-w-0 overflow-hidden"
            >
              <section className="flex h-full min-h-0 min-w-0 flex-col border-b">
                <div className="flex h-8 shrink-0 items-center justify-between border-b px-2">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Running Plugins
                  </div>
                  <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
                    {runningPluginRows.length}
                  </Badge>
                </div>
                <div className="min-h-0 flex-1 overflow-auto">
                  {runningPluginRows.length > 0 ? (
                    <div className="divide-y">
                      {runningPluginRows.map(({ datasource, job, pluginId, run }) => {
                        const status =
                          run?.status === "cancelling"
                            ? "cancelling"
                            : job?.status ?? "queued";
                        const datasourceLabel =
                          datasource?.name ??
                          job?.datasourceId ??
                          run?.datasourceId ??
                          "Unknown datasource";

                        return (
                          <div
                            key={job?.id ?? `${run?.runId ?? "queued"}:${pluginId}`}
                            className="space-y-1 px-2 py-1.5 text-xs"
                          >
                            <div className="flex min-w-0 items-center gap-2">
                              <Badge
                                variant="secondary"
                                className={cn(
                                  "h-5 rounded-sm text-[11px]",
                                  job ? getStatusBadgeClassName(job.status) : undefined,
                                )}
                              >
                                {status}
                              </Badge>
                              <span className="min-w-0 truncate font-medium">
                                {getPluginLabel(pluginMap, pluginId)}
                              </span>
                            </div>
                            <div className="truncate text-[11px] text-muted-foreground">
                              {datasourceLabel}
                            </div>
                            <div className="flex items-center justify-between gap-2">
                              <span className="truncate text-[11px] text-muted-foreground">
                                {job ? formatJobTime(job.startedAt) : "Waiting to start"}
                              </span>
                              <Button
                                type="button"
                                variant="outline"
                                size="xs"
                                className="h-6 rounded-sm px-2 text-[11px]"
                                disabled={!run || run.status === "cancelling"}
                                onClick={() => {
                                  if (run) {
                                    void cancelPluginRun(run.runId);
                                  }
                                }}
                              >
                                {run?.status === "cancelling"
                                  ? "Cancelling"
                                  : run
                                    ? "Cancel"
                                    : "No run id"}
                              </Button>
                            </div>
                            {job?.error && (
                              <div className="line-clamp-2 text-[11px] text-destructive">
                                {job.error}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  ) : (
                    <div className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
                      No plugins are currently running.
                    </div>
                  )}
                </div>
              </section>
            </ResizablePanel>

            <ResizableHandle withHandle />

            <ResizablePanel
              defaultSize="50%"
              minSize="24%"
              className="min-h-0 min-w-0 overflow-hidden"
            >
              <section className="flex h-full min-h-0 min-w-0 flex-col">
                <div className="flex h-8 shrink-0 items-center justify-between border-b px-2">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Plugin Logs
                  </div>
                  <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
                    {logs.length}
                  </Badge>
                </div>
                <div className="min-h-0 flex-1 overflow-auto bg-muted/20 font-mono text-[11px]">
                  {logs.length > 0 ? (
                    <div className="divide-y">
                      {logs.map((log) => (
                        <div key={log.id} className="px-2 py-1">
                          <div className="flex min-w-0 items-center gap-2">
                            <span
                              className={cn(
                                "shrink-0 uppercase",
                                log.level === "error"
                                  ? "text-destructive"
                                  : "text-muted-foreground",
                              )}
                            >
                              {log.level}
                            </span>
                            <span className="truncate text-muted-foreground">
                              {formatJobTime(log.createdAt)}
                            </span>
                          </div>
                          <div className="whitespace-pre-wrap break-words">
                            [{getPluginLabel(pluginMap, log.pluginId)}] {log.message}
                          </div>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="grid h-full place-items-center px-3 text-center font-sans text-xs text-muted-foreground">
                      No plugin logs have been written yet.
                    </div>
                  )}
                </div>
              </section>
            </ResizablePanel>
          </ResizablePanelGroup>
        </ResizablePanel>
      </ResizablePanelGroup>

      <Dialog
        open={isCreateDialogOpen}
        onOpenChange={(isOpen) => {
          setIsCreateDialogOpen(isOpen);

          if (!isOpen) {
            resetCreatePluginForm();
          }
        }}
      >
        <DialogContent className="w-[calc(100vw-2rem)] max-w-3xl rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">Add Python Plugin</DialogTitle>
            <DialogDescription className="text-xs">
              Create plugin.py and plugin.toml from manual fields or a folder name plus pasted TOML.
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void createPlugin();
            }}
          >
            <div className="max-h-[min(34rem,calc(100vh-12rem))] overflow-auto px-3 py-3">
              <label className="mb-3 block space-y-1 text-xs">
                <span className="text-muted-foreground">
                  Organization folder (optional)
                </span>
                <Input
                  className="h-8 rounded-sm font-mono text-xs"
                  value={newPluginOrganizationFolder}
                  placeholder="VLEAPP/Ford"
                  onChange={(event) =>
                    setNewPluginOrganizationFolder(event.target.value)
                  }
                />
                <span className="block text-[10px] text-muted-foreground">
                  Nested folders are created automatically.
                </span>
              </label>
              <Tabs
                value={createPluginMode}
                onValueChange={handleCreatePluginModeChange}
              >
                <TabsList className="h-8 rounded-sm">
                  <TabsTrigger value="manual" className="h-7 rounded-sm text-xs">
                    Manual Entry
                  </TabsTrigger>
                  <TabsTrigger
                    value="automatic"
                    className="h-7 rounded-sm text-xs"
                  >
                    Automatic Entry
                  </TabsTrigger>
                </TabsList>

                <TabsContent value="manual" className="mt-3 space-y-3">
                  <div className="grid gap-2 sm:grid-cols-2">
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Plugin ID</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginId}
                        autoFocus
                        placeholder="ford-phonebook"
                        onChange={(event) => setNewPluginId(event.target.value)}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Name</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginName}
                        placeholder="Ford Phonebook"
                        onChange={(event) => setNewPluginName(event.target.value)}
                      />
                    </label>
                    <label className="space-y-1 text-xs sm:col-span-2">
                      <span className="text-muted-foreground">Description</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginDescription}
                        placeholder="Extracts infotainment phonebook records."
                        onChange={(event) =>
                          setNewPluginDescription(event.target.value)
                        }
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Author</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginAuthor}
                        placeholder="Your Name"
                        onChange={(event) => setNewPluginAuthor(event.target.value)}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Version</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginVersion}
                        placeholder="1.0.0"
                        onChange={(event) => setNewPluginVersion(event.target.value)}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Type</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginType}
                        placeholder="contacts"
                        onChange={(event) => setNewPluginType(event.target.value)}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Entry file</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginEntry}
                        placeholder="plugin.py"
                        onChange={(event) => setNewPluginEntry(event.target.value)}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Function</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginFunction}
                        placeholder="run"
                        onChange={(event) =>
                          setNewPluginFunction(event.target.value)
                        }
                      />
                    </label>
                  </div>

                  <div className="space-y-1">
                    <div className="text-xs text-muted-foreground">Target</div>
                    <div className="flex flex-wrap gap-1">
                      {createPluginTargets.map((target) => (
                        <Button
                          key={target.value}
                          type="button"
                          variant={
                            newPluginTarget === target.value
                              ? "secondary"
                              : "outline"
                          }
                          size="xs"
                          className="h-7 rounded-sm px-2 text-[11px]"
                          onClick={() => setNewPluginTarget(target.value)}
                        >
                          {target.label}
                        </Button>
                      ))}
                    </div>
                  </div>

                  <div className="space-y-1">
                    <div className="text-xs text-muted-foreground">Mode</div>
                    <div className="flex flex-wrap gap-1">
                      {createPluginModes.map((mode) => (
                        <Button
                          key={mode.value}
                          type="button"
                          variant={
                            newPluginRunMode === mode.value
                              ? "secondary"
                              : "outline"
                          }
                          size="xs"
                          className="h-7 rounded-sm px-2 text-[11px]"
                          onClick={() => setNewPluginRunMode(mode.value)}
                        >
                          {mode.label}
                        </Button>
                      ))}
                    </div>
                  </div>

                  <div className="grid gap-2 sm:grid-cols-2">
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Path glob</span>
                      <textarea
                        className="min-h-20 w-full resize-y rounded-sm border bg-transparent px-2 py-1.5 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                        value={newPluginPathGlob}
                        placeholder={"*/phonebook.db\n*/contacts*.json"}
                        onChange={(event) =>
                          setNewPluginPathGlob(event.target.value)
                        }
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Path regex</span>
                      <textarea
                        className="min-h-20 w-full resize-y rounded-sm border bg-transparent px-2 py-1.5 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                        value={newPluginPathRegex}
                        placeholder="(?i).*phonebook.*"
                        onChange={(event) =>
                          setNewPluginPathRegex(event.target.value)
                        }
                      />
                    </label>
                  </div>
                </TabsContent>

                <TabsContent value="automatic" className="mt-3 space-y-2">
                  <label className="grid grid-cols-[7rem_minmax(0,1fr)] items-center gap-2 text-xs">
                    <span className="text-muted-foreground">Folder name</span>
                    <Input
                      className="h-8 rounded-sm text-xs"
                      value={newPluginName}
                      onChange={(event) => setNewPluginName(event.target.value)}
                    />
                  </label>
                  <label className="space-y-1 text-xs">
                    <span className="text-muted-foreground">TOML Details</span>
                    <textarea
                      className="min-h-72 w-full resize-y rounded-sm border bg-transparent px-2 py-1.5 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                      value={newPluginTomlDetails}
                      onChange={(event) =>
                        setNewPluginTomlDetails(event.target.value)
                      }
                      spellCheck={false}
                    />
                  </label>
                </TabsContent>
              </Tabs>
            </div>
            <DialogFooter className="border-t px-3 py-2">
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="h-7 rounded-sm px-2 text-xs"
                disabled={creatingPlugin}
                onClick={() => {
                  setIsCreateDialogOpen(false);
                  resetCreatePluginForm();
                }}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                size="xs"
                className="h-7 rounded-sm px-2 text-xs"
                disabled={creatingPlugin}
              >
                <Plus className="size-3.5" aria-hidden="true" />
                {creatingPlugin ? "Creating" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={isCreateFolderDialogOpen}
        onOpenChange={(isOpen) => {
          setIsCreateFolderDialogOpen(isOpen);
          if (!isOpen) {
            setNewOrganizationFolder("");
          }
        }}
      >
        <DialogContent className="w-[calc(100vw-2rem)] max-w-md rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">New Plugin Folder</DialogTitle>
            <DialogDescription className="text-xs">
              Create a virtual organization folder under the Python plugin
              directory.
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void createOrganizationFolder();
            }}
          >
            <div className="space-y-1 px-3 py-3">
              <label className="space-y-1 text-xs">
                <span className="text-muted-foreground">Folder path</span>
                <Input
                  autoFocus
                  className="h-8 rounded-sm font-mono text-xs"
                  value={newOrganizationFolder}
                  placeholder="VLEAPP/Ford"
                  onChange={(event) =>
                    setNewOrganizationFolder(event.target.value)
                  }
                />
              </label>
            </div>
            <DialogFooter className="border-t px-3 py-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-8 rounded-sm text-xs"
                onClick={() => setIsCreateFolderDialogOpen(false)}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                size="sm"
                className="h-8 rounded-sm text-xs"
                disabled={creatingOrganizationFolder}
              >
                {creatingOrganizationFolder ? "Creating..." : "Create Folder"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={isBulkMoveDialogOpen}
        onOpenChange={(isOpen) => {
          if (bulkMoveProgress) {
            return;
          }
          setIsBulkMoveDialogOpen(isOpen);
          if (!isOpen) {
            setBulkMoveSource("");
            setBulkMovePluginIds([]);
            setBulkMoveDestination("");
          }
        }}
      >
        <DialogContent className="w-[calc(100vw-2rem)] max-w-xl rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">Move Python Plugins</DialogTitle>
            <DialogDescription className="text-xs">
              Choose a folder to view, select its plugins, then choose where to
              move them.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 px-3 py-3">
            <div className="grid gap-3 sm:grid-cols-2">
              <label className="block space-y-1 text-xs">
                <span className="text-muted-foreground">View plugins in</span>
                <select
                  className="h-8 w-full rounded-sm border bg-background px-2 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                  value={bulkMoveSource}
                  disabled={bulkMoveProgress !== null}
                  onChange={(event) => {
                    setBulkMoveSource(event.target.value);
                    setBulkMovePluginIds([]);
                  }}
                >
                  <option value="">Plugin root</option>
                  {pluginFolders.map((folder) => (
                    <option key={folder} value={folder}>
                      {folder}
                    </option>
                  ))}
                </select>
              </label>
              <label className="block space-y-1 text-xs">
                <span className="text-muted-foreground">Destination folder</span>
                <select
                  className="h-8 w-full rounded-sm border bg-background px-2 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                  value={bulkMoveDestination}
                  disabled={bulkMoveProgress !== null}
                  onChange={(event) =>
                    setBulkMoveDestination(event.target.value)
                  }
                >
                  <option value="">Plugin root</option>
                  {pluginFolders.map((folder) => (
                    <option key={folder} value={folder}>
                      {folder}
                    </option>
                  ))}
                </select>
              </label>
            </div>

            <div className="overflow-hidden rounded-sm border">
              <label className="flex cursor-pointer items-center gap-2 border-b bg-muted px-2 py-2 text-xs font-medium">
                <Checkbox
                  checked={
                    visibleBulkMovePlugins.length > 0 &&
                    bulkMovePluginIds.length === visibleBulkMovePlugins.length
                      ? true
                      : bulkMovePluginIds.length > 0
                        ? "indeterminate"
                        : false
                  }
                  disabled={bulkMoveProgress !== null}
                  onCheckedChange={(checked) => {
                    setBulkMovePluginIds(
                      checked === true
                        ? visibleBulkMovePlugins.map((plugin) => plugin.id)
                        : [],
                    );
                  }}
                />
                <span>Select all in this folder</span>
                <span className="ml-auto text-[10px] text-muted-foreground">
                  {bulkMovePluginIds.length} selected of{" "}
                  {visibleBulkMovePlugins.length}
                </span>
              </label>
              <div className="max-h-72 divide-y overflow-y-auto">
                {visibleBulkMovePlugins.map((plugin) => {
                  const selected = bulkMovePluginIds.includes(plugin.id);

                  return (
                    <label
                      key={plugin.id}
                      className="flex cursor-pointer items-center gap-2 px-2 py-2 text-xs hover:bg-accent"
                    >
                      <Checkbox
                        checked={selected}
                        disabled={bulkMoveProgress !== null}
                        onCheckedChange={(checked) =>
                          toggleBulkMovePlugin(plugin.id, checked === true)
                        }
                      />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-medium">
                          {plugin.name}
                        </span>
                        <span className="block truncate font-mono text-[10px] text-muted-foreground">
                          {plugin.organizationFolder || "Plugin root"}
                        </span>
                      </span>
                    </label>
                  );
                })}
                {visibleBulkMovePlugins.length === 0 && (
                  <div className="px-3 py-8 text-center text-xs text-muted-foreground">
                    No movable plugins are directly inside this folder.
                  </div>
                )}
              </div>
            </div>
          </div>
          <DialogFooter className="border-t px-3 py-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 rounded-sm text-xs"
              disabled={bulkMoveProgress !== null}
              onClick={() => setIsBulkMoveDialogOpen(false)}
            >
              Cancel
            </Button>
            <Button
              type="button"
              size="sm"
              className="h-8 rounded-sm text-xs"
              disabled={
                bulkMovePluginIds.length === 0 ||
                bulkMoveProgress !== null ||
                bulkMoveDestination === bulkMoveSource
              }
              onClick={() => void bulkMovePlugins()}
            >
              <FolderInput className="size-3.5" aria-hidden="true" />
              {bulkMoveProgress
                ? `Moving ${bulkMoveProgress.completed}/${bulkMoveProgress.total}`
                : `Move ${bulkMovePluginIds.length} Plugin${bulkMovePluginIds.length === 1 ? "" : "s"}`}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={movingPlugin !== null}
        onOpenChange={(isOpen) => {
          if (!isOpen && !isMovingPlugin) {
            setMovingPlugin(null);
          }
        }}
      >
        <DialogContent className="w-[calc(100vw-2rem)] max-w-md rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">Move Python Plugin</DialogTitle>
            <DialogDescription className="text-xs">
              Choose where to move {movingPlugin?.name || "this plugin"}.
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-80 space-y-1 overflow-y-auto px-3 py-3">
            <Button
              type="button"
              variant="ghost"
              className="h-9 w-full justify-start rounded-sm px-2 font-mono text-xs"
              disabled={isMovingPlugin || !movingPlugin?.organizationFolder}
              onClick={() => void movePlugin("")}
            >
              <FolderOpen className="size-4" aria-hidden="true" />
              Plugin root
              {!movingPlugin?.organizationFolder && (
                <span className="ml-auto text-[10px] text-muted-foreground">
                  Current
                </span>
              )}
            </Button>
            {pluginFolders.map((folder) => {
              const isCurrent = movingPlugin?.organizationFolder === folder;

              return (
                <Button
                  key={folder}
                  type="button"
                  variant="ghost"
                  className="h-9 w-full justify-start rounded-sm px-2 font-mono text-xs"
                  disabled={isMovingPlugin || isCurrent}
                  onClick={() => void movePlugin(folder)}
                >
                  <Folder className="size-4" aria-hidden="true" />
                  <span className="truncate">{folder}</span>
                  {isCurrent && (
                    <span className="ml-auto text-[10px] text-muted-foreground">
                      Current
                    </span>
                  )}
                </Button>
              );
            })}
            {pluginFolders.length === 0 && (
              <div className="px-2 py-4 text-center text-xs text-muted-foreground">
                No organization folders have been created yet.
              </div>
            )}
          </div>
          <DialogFooter className="border-t px-3 py-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 rounded-sm text-xs"
              disabled={isMovingPlugin}
              onClick={() => setMovingPlugin(null)}
            >
              Cancel
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

async function copyTextToClipboard(text: string) {
  if (navigator.clipboard) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textArea = document.createElement("textarea");

  textArea.value = text;
  textArea.setAttribute("readonly", "");
  textArea.style.position = "fixed";
  textArea.style.left = "-9999px";
  document.body.appendChild(textArea);
  textArea.select();

  try {
    const copied = document.execCommand("copy");

    if (!copied) {
      throw new Error("Clipboard copy was not allowed.");
    }
  } finally {
    document.body.removeChild(textArea);
  }
}
