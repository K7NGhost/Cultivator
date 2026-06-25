import { invoke } from "@tauri-apps/api/core";

import type {
  CreatedPythonPlugin,
  PluginJobRecord,
  PluginLogRecord,
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
  request:
    | string
    | {
        manifest?: {
          id: string;
          name: string;
          description: string;
          type: string;
          target:
            | "ios"
            | "android"
            | "windows"
            | "macos"
            | "infotainment"
            | "other";
          mode: "each_file" | "path_glob" | "path_regex";
          pathGlob?: string[];
          pathRegex?: string;
          entry: string;
          function: string;
        };
        manifestToml?: string;
        name?: string;
      },
): Promise<CreatedPythonPlugin> {
  const createRequest =
    typeof request === "string" ? { name: request } : request;

  return invoke<CreatedPythonPlugin>("create_python_plugin", {
    request: createRequest,
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
  runId?: string;
}): Promise<PluginRunSummary> {
  return invoke<PluginRunSummary>("run_datasource_plugins", input);
}

export async function listPluginLogs(
  caseDatabasePath: string,
  limit = 200,
): Promise<PluginLogRecord[]> {
  return invoke<PluginLogRecord[]>("list_plugin_logs", {
    caseDatabasePath,
    limit,
  });
}

export async function cancelDatasourcePluginRun(runId: string): Promise<boolean> {
  return invoke<boolean>("cancel_plugin_run", { runId });
}
