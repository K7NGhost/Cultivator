import { useEffect, useMemo, useState } from "react";
import { AlertCircle, Play, RefreshCw } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useCases } from "@/features/cases/case-provider";
import { listDataSources } from "@/features/datasources/dataSourceRepository";
import type { DataSourceRecord } from "@/features/datasources/types";
import {
  listPluginJobs,
  listPythonPlugins,
  runDatasourcePlugins,
} from "@/features/plugins/pluginRepository";
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

    try {
      await runDatasourcePlugins({
        caseDatabasePath: activeCase.databasePath,
        caseFolderPath: activeCase.folderPath,
        datasourceId: datasource.id,
      });
      await refreshPluginsPage();
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    } finally {
      setRunningDatasourceId(null);
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
        <section className="min-h-0 min-w-0 border-r">
          <div className="flex h-8 items-center justify-between border-b px-2">
            <div className="text-xs font-medium uppercase text-muted-foreground">
              Datasource Jobs
            </div>
            <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
              {loadState.isLoading ? "Loading" : "Ready"}
            </Badge>
          </div>

          <Table
            containerClassName="h-[calc(100%-2rem)] overflow-auto"
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
    </div>
  );
}
