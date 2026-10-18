import type { DataSourcePlugin } from "@/features/datasources";

export type PythonPlugin = DataSourcePlugin;

export type PythonRuntimeStatus = {
  uvAvailable: boolean;
  ready: boolean;
  directory: string;
  python: string;
};

export type CreatedPythonPlugin = {
  id: string;
  directory: string;
  manifestPath: string;
  scriptPath: string;
  openedInVscode: boolean;
};

export type PluginBundleExportResult = {
  archivePath: string;
  pluginCount: number;
  fileCount: number;
  byteCount: number;
};

export type PluginBundleImportResult = {
  archivePath: string;
  pluginCount: number;
  fileCount: number;
  byteCount: number;
};

export type PluginJobRecord = {
  id: string;
  caseId: string;
  datasourceId: string;
  pluginId: string;
  status: "queued" | "paused" | "running" | "cancelling" | "cancelled" | "complete" | "failed" | "interrupted";
  runId: string;
  queuedAt: string;
  priority: number;
  completed: number;
  total: number;
  message: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  error: string | null;
};

export type SubmitPluginRunInput = {
  caseDatabasePath: string;
  caseFolderPath: string;
  datasourceId: string;
  pluginIds?: string[];
  pluginOptions?: Record<string, Record<string, string>>;
  runId?: string;
  priority?: number;
};

export type SubmittedPluginRun = { runId: string; jobs: PluginJobRecord[] };
export type PluginRunSnapshot = { runId: string; finished: boolean; summary: PluginRunSummary };

export function isActivePluginJob(job: PluginJobRecord) {
  return ["queued", "paused", "running", "cancelling"].includes(job.status);
}

export type PluginLogRecord = {
  id: string;
  jobId: string;
  pluginId: string;
  level: string;
  message: string;
  createdAt: string;
};

export type PluginRunSummary = {
  datasourceId: string;
  datasourcePathsUpdated: boolean;
  jobs: PluginJobRecord[];
};
