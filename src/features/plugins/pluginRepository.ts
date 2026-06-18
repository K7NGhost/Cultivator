import { invoke } from "@tauri-apps/api/core";

import type {
  CreatedPythonPlugin,
  PluginJobRecord,
  PluginRunSummary,
  PythonPlugin,
} from "@/features/plugins/types";

export async function listPythonPlugins(): Promise<PythonPlugin[]> {
  return invoke<PythonPlugin[]>("list_python_plugins");
}

export async function openPythonPluginFolder(): Promise<void> {
  await invoke("open_python_plugin_directory");
}

export async function openPythonPluginFolderInVscode(): Promise<void> {
  await invoke("open_python_plugin_directory_in_vscode");
}

export async function openPythonApiGuide(): Promise<void> {
  await invoke("open_python_api_guide");
}

export async function createPythonPlugin(
  name: string,
): Promise<CreatedPythonPlugin> {
  return invoke<CreatedPythonPlugin>("create_python_plugin", {
    request: { name },
  });
}

export async function deletePythonPlugin(pluginId: string): Promise<void> {
  await invoke("delete_python_plugin", {
    request: { pluginId },
  });
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
  pluginIds?: string[];
}): Promise<PluginRunSummary> {
  return invoke<PluginRunSummary>("run_datasource_plugins", input);
}
