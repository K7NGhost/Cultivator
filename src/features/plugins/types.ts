import type { DataSourcePlugin } from "@/features/datasources/types";

export type PythonPlugin = DataSourcePlugin;

export type CreatedPythonPlugin = {
  id: string;
  directory: string;
  manifestPath: string;
  scriptPath: string;
  openedInVscode: boolean;
};

export type PluginJobRecord = {
  id: string;
  caseId: string;
  datasourceId: string;
  pluginId: string;
  status: "running" | "complete" | "failed";
  startedAt: string;
  finishedAt: string | null;
  error: string | null;
};

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
