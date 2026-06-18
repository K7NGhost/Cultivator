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

export type PluginRunSummary = {
  datasourceId: string;
  jobs: PluginJobRecord[];
};
