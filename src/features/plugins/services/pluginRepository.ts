import { invoke } from "@tauri-apps/api/core";

import type {
  CreatedPythonPlugin,
  PluginBundleExportResult,
  PluginBundleImportResult,
  PluginJobRecord,
  PluginLogRecord,
  PluginRunSummary,
  PythonPlugin,
  PythonRuntimeStatus,
  SubmitPluginRunInput,
  SubmittedPluginRun,
  PluginRunSnapshot,
} from "../model/types";
import { waitForPluginProgressListener } from "../utils/pluginToasts";

export const archiveExtractorPluginId = "archive-extractor";

export function getPythonRuntimeStatus(): Promise<PythonRuntimeStatus> {
  return invoke("python_runtime_status");
}

export function preparePythonRuntime(): Promise<PythonRuntimeStatus> {
  return invoke("prepare_python_runtime");
}

export function pluginRunUpdatedDatasourcePaths(summary: PluginRunSummary) {
  return (
    summary.datasourcePathsUpdated ||
    summary.jobs.some(
      (job) =>
        job.pluginId === archiveExtractorPluginId && job.status === "complete",
    )
  );
}

export async function listPythonPlugins(): Promise<PythonPlugin[]> {
  return invoke<PythonPlugin[]>("list_python_plugins");
}

export async function listPythonPluginFolders(): Promise<string[]> {
  return invoke<string[]>("list_python_plugin_folders");
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
          author: string;
          version: string;
          python?: string;
          dependencies?: string[];
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
          archivePathGlob?: string[];
          pathRegex?: string;
          entry: string;
          function: string;
        };
        folderName?: string;
        organizationFolder?: string;
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

export async function exportPythonPlugins(
  archivePath: string,
): Promise<PluginBundleExportResult> {
  return invoke<PluginBundleExportResult>("export_python_plugins", {
    request: { archivePath },
  });
}

export async function importPythonPlugins(
  archivePath: string,
): Promise<PluginBundleImportResult> {
  return invoke<PluginBundleImportResult>("import_python_plugins", {
    request: { archivePath },
  });
}

export async function listPluginJobs(
  caseDatabasePath: string,
  pagination: { limit?: number; offset?: number } = {},
): Promise<PluginJobRecord[]> {
  return invoke<PluginJobRecord[]>("list_plugin_jobs", {
    caseDatabasePath,
    ...pagination,
  });
}

export async function submitDatasourcePlugins(input: SubmitPluginRunInput): Promise<SubmittedPluginRun> {
  await waitForPluginProgressListener();
  return invoke<SubmittedPluginRun>("submit_plugin_run", { request: input });
}

export async function waitForPluginRun(caseDatabasePath: string, runId: string): Promise<PluginRunSummary> {
  // SQLite is authoritative, including after a missed event or a webview reload.
  while (true) {
    const snapshot = await invoke<PluginRunSnapshot>("get_plugin_run", { caseDatabasePath, runId });
    if (snapshot.finished) return snapshot.summary;
    await new Promise<void>((resolve) => window.setTimeout(resolve, 1000));
  }
}

export async function runDatasourcePlugins(input: SubmitPluginRunInput): Promise<PluginRunSummary> {
  const submission = await submitDatasourcePlugins(input);
  return waitForPluginRun(input.caseDatabasePath, submission.runId);
}

export async function pauseDatasourcePluginRun(runId: string, paused: boolean): Promise<boolean> {
  return invoke<boolean>("pause_plugin_run", { runId, paused });
}

export async function movePythonPlugin(
  pluginId: string,
  organizationFolder: string,
): Promise<string> {
  return invoke<string>("move_python_plugin", {
    request: { pluginId, organizationFolder },
  });
}

export async function createPythonPluginFolder(folder: string): Promise<string> {
  return invoke<string>("create_python_plugin_folder", {
    request: { folder },
  });
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

export function cancelPluginJob(jobId: string): Promise<boolean> {
  return invoke<boolean>("cancel_plugin_job", { jobId });
}

export function pausePluginJob(jobId: string, paused: boolean): Promise<boolean> {
  return invoke<boolean>("pause_plugin_job", { jobId, paused });
}
