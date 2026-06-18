import { useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Copy,
  FolderOpen,
  Plus,
  Play,
  RefreshCw,
  Trash2,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
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
import { listDataSources } from "@/features/datasources/dataSourceRepository";
import type { DataSourceRecord } from "@/features/datasources/types";
import {
  createPythonPlugin,
  deletePythonPlugin,
  listPluginJobs,
  listPythonPlugins,
  openPythonPluginFolder,
  runDatasourcePlugins,
} from "@/features/plugins/pluginRepository";
import {
  showPluginRunFailedToast,
  showPluginRunFinishedToasts,
  showPluginRunStartedToast,
} from "@/features/plugins/pluginToasts";
import type { PluginJobRecord, PythonPlugin } from "@/features/plugins/types";
import { cn } from "@/lib/utils";

type LoadState = {
  error: string | null;
  isLoading: boolean;
};

function getErrorMessage(caughtError: unknown) {
  return caughtError instanceof Error ? caughtError.message : String(caughtError);
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

function getPluginLabel(pluginMap: Map<string, PythonPlugin>, pluginId: string) {
  return pluginMap.get(pluginId)?.name ?? pluginId;
}

export function PluginsPage() {
  const { activeCase } = useCases();
  const [datasources, setDatasources] = useState<DataSourceRecord[]>([]);
  const [plugins, setPlugins] = useState<PythonPlugin[]>([]);
  const [jobs, setJobs] = useState<PluginJobRecord[]>([]);
  const [runningDatasourceId, setRunningDatasourceId] = useState<string | null>(
    null,
  );
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [newPluginName, setNewPluginName] = useState("");
  const [creatingPlugin, setCreatingPlugin] = useState(false);
  const [deletingPluginId, setDeletingPluginId] = useState<string | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied">("idle");
  const [loadState, setLoadState] = useState<LoadState>({
    error: null,
    isLoading: false,
  });
  const pluginMap = useMemo(() => {
    return new Map(plugins.map((plugin) => [plugin.id, plugin]));
  }, [plugins]);

  async function refreshPluginsPage() {
    if (!activeCase) {
      setDatasources([]);
      setPlugins([]);
      setJobs([]);
      setLoadState({ error: null, isLoading: false });
      return;
    }

    setLoadState({ error: null, isLoading: true });

    try {
      const [nextDatasources, nextPlugins, nextJobs] = await Promise.all([
        listDataSources(activeCase.databasePath, activeCase.id),
        listPythonPlugins(),
        listPluginJobs(activeCase.databasePath),
      ]);

      setDatasources(nextDatasources);
      setPlugins(nextPlugins);
      setJobs(nextJobs);
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

    setRunningDatasourceId(datasource.id);
    setLoadState((currentState) => ({ ...currentState, error: null }));
    const toastId = showPluginRunStartedToast({
      datasourceName: datasource.name,
      pluginCount: datasource.pluginIds.length,
    });

    try {
      const summary = await runDatasourcePlugins({
        caseDatabasePath: activeCase.databasePath,
        caseFolderPath: activeCase.folderPath,
        datasourceId: datasource.id,
      });
      showPluginRunFinishedToasts({
        datasourceName: datasource.name,
        pluginMap,
        summary,
        toastId,
      });
      await refreshPluginsPage();
    } catch (caughtError) {
      showPluginRunFailedToast({
        datasourceName: datasource.name,
        error: caughtError,
        toastId,
      });
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    } finally {
      setRunningDatasourceId(null);
    }
  }

  async function createPlugin() {
    const pluginName = newPluginName.trim();

    if (!pluginName) {
      setLoadState({ error: "Plugin name is required.", isLoading: false });
      return;
    }

    setCreatingPlugin(true);
    setLoadState((currentState) => ({ ...currentState, error: null }));

    try {
      await createPythonPlugin(pluginName);
      setNewPluginName("");
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

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
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

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_20rem]">
        <section className="flex min-h-0 min-w-0 flex-col border-r">
          <div className="flex h-8 shrink-0 items-center justify-between border-b px-2">
            <div className="text-xs font-medium uppercase text-muted-foreground">
              Installed Plugins
            </div>
            <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
              {plugins.length}
            </Badge>
          </div>

          <Table
            containerClassName="max-h-56 shrink-0 overflow-auto border-b"
            className="min-w-[760px] table-fixed text-xs"
          >
            <TableHeader className="sticky top-0 z-10 bg-muted">
              <TableRow className="hover:bg-muted">
                <TableHead className="h-7 w-[190px] px-2">Plugin</TableHead>
                <TableHead className="h-7 w-[110px] px-2">Mode</TableHead>
                <TableHead className="h-7 w-[260px] px-2">Description</TableHead>
                <TableHead className="h-7 w-[110px] px-2">Entry</TableHead>
                <TableHead className="h-7 w-[90px] px-2">Action</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {plugins.map((plugin) => {
                const isDeleting = deletingPluginId === plugin.id;

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
                      <Button
                        type="button"
                        variant="ghost"
                        size="xs"
                        className="h-7 rounded-sm px-2 text-xs text-destructive hover:text-destructive"
                        disabled={isDeleting}
                        onClick={() => {
                          void deletePlugin(plugin);
                        }}
                      >
                        <Trash2 className="size-3.5" aria-hidden="true" />
                        {isDeleting ? "Deleting" : "Delete"}
                      </Button>
                    </TableCell>
                  </TableRow>
                );
              })}
              {plugins.length === 0 && (
                <TableRow>
                  <TableCell
                    colSpan={5}
                    className="h-16 text-center text-xs text-muted-foreground"
                  >
                    No Python plugins are installed.
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>

          <div className="flex h-8 items-center justify-between border-b px-2">
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
                const isRunning = runningDatasourceId === datasource.id;

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
                        {isRunning ? "Running" : "Run plugins"}
                      </Button>
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

        <aside className="min-h-0 min-w-0">
          <div className="flex h-8 items-center border-b px-2 text-xs font-medium uppercase text-muted-foreground">
            Recent Jobs
          </div>
          <div className="h-[calc(100%-2rem)] overflow-auto">
            {jobs.length > 0 ? (
              <div className="divide-y">
                {jobs.slice(0, 25).map((job) => (
                  <div key={job.id} className="space-y-1 px-2 py-1.5 text-xs">
                    <div className="flex min-w-0 items-center gap-2">
                      <Badge
                        variant="secondary"
                        className={cn(
                          "h-5 rounded-sm text-[11px]",
                          getStatusBadgeClassName(job.status),
                        )}
                      >
                        {job.status}
                      </Badge>
                      <span className="min-w-0 truncate font-medium">
                        {getPluginLabel(pluginMap, job.pluginId)}
                      </span>
                    </div>
                    <div className="truncate text-[11px] text-muted-foreground">
                      {formatJobTime(job.startedAt)}
                    </div>
                    {job.error && (
                      <div className="line-clamp-2 text-[11px] text-destructive">
                        {job.error}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <div className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
                No plugin jobs have been run.
              </div>
            )}
          </div>
        </aside>
      </div>

      <Dialog open={isCreateDialogOpen} onOpenChange={setIsCreateDialogOpen}>
        <DialogContent className="max-w-md rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">Add Python Plugin</DialogTitle>
            <DialogDescription className="text-xs">
              Create a plugin folder with plugin.py and plugin.toml.
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void createPlugin();
            }}
          >
            <div className="px-3 py-3">
              <Input
                className="h-8 rounded-sm text-xs"
                value={newPluginName}
                autoFocus
                placeholder="Plugin name"
                onChange={(event) => setNewPluginName(event.target.value)}
              />
            </div>
            <DialogFooter className="border-t px-3 py-2">
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="h-7 rounded-sm px-2 text-xs"
                disabled={creatingPlugin}
                onClick={() => setIsCreateDialogOpen(false)}
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
