import { invoke } from "@tauri-apps/api/core";

import type {
  PluginJobRecord,
  PluginRunSummary,
  PythonPlugin,
} from "@/features/plugins/types";

export async function listPythonPlugins(): Promise<PythonPlugin[]> {
  return invoke<PythonPlugin[]>("list_python_plugins");
}

export async function listPluginJobs(
  caseDatabasePath: string,
): Promise<PluginJobRecord[]> {
  return invoke<PluginJobRecord[]>("list_plugin_jobs", {
    caseDatabasePath,
  });
}

export async function runDatasourcePlugins(input: {
  caseDatabasePath: string;
  caseFolderPath: string;
  datasourceId: string;
}): Promise<PluginRunSummary> {
  return invoke<PluginRunSummary>("run_datasource_plugins", input);
}
