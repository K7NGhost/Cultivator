import type { DataSourcePlugin } from "@/features/datasources/types";

export type PythonPlugin = DataSourcePlugin;

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
