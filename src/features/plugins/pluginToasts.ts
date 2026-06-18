import { toast, type Id } from "react-toastify";

import type {
  PluginRunSummary,
  PythonPlugin,
} from "@/features/plugins/types";

export function showPluginRunStartedToast(input: {
  datasourceName: string;
  pluginCount: number;
}) {
  return toast.loading(
    `Running ${input.pluginCount.toLocaleString()} plugin${
      input.pluginCount === 1 ? "" : "s"
    } on ${input.datasourceName}`,
    {
      closeButton: true,
      closeOnClick: false,
    },
  );
}

export function showPluginRunFinishedToasts(input: {
  datasourceName: string;
  pluginMap: Map<string, PythonPlugin>;
  summary: PluginRunSummary;
  toastId?: Id;
}) {
  const completedJobs = input.summary.jobs.filter(
    (job) => job.status === "complete",
  );
  const failedJobs = input.summary.jobs.filter((job) => job.status === "failed");
  const runningJobs = input.summary.jobs.filter(
    (job) => job.status === "running",
  );

  if (input.toastId) {
    toast.update(input.toastId, {
      render: `Finished plugin run on ${input.datasourceName}`,
      type: failedJobs.length > 0 ? "warning" : "success",
      isLoading: false,
      autoClose: false,
      closeButton: true,
      closeOnClick: false,
    });
  }

  for (const job of completedJobs) {
    toast.success(
      `${getPluginLabel(input.pluginMap, job.pluginId)} completed on ${
        input.datasourceName
      }`,
      { autoClose: false, closeButton: true, closeOnClick: false },
    );
  }

  for (const job of failedJobs) {
    toast.error(
      `${getPluginLabel(input.pluginMap, job.pluginId)} failed: ${
        job.error ?? "Unknown error"
      }`,
      { autoClose: false, closeButton: true, closeOnClick: false },
    );
  }

  for (const job of runningJobs) {
    toast.info(
      `${getPluginLabel(input.pluginMap, job.pluginId)} is still marked running.`,
      { autoClose: false, closeButton: true, closeOnClick: false },
    );
  }
}

export function showPluginRunFailedToast(input: {
  datasourceName: string;
  error: unknown;
  toastId?: Id;
}) {
  const message =
    input.error instanceof Error ? input.error.message : String(input.error);

  if (input.toastId) {
    toast.update(input.toastId, {
      render: `Plugin run failed on ${input.datasourceName}: ${message}`,
      type: "error",
      isLoading: false,
      autoClose: false,
      closeButton: true,
      closeOnClick: false,
    });
    return;
  }

  toast.error(`Plugin run failed on ${input.datasourceName}: ${message}`, {
    autoClose: false,
    closeButton: true,
    closeOnClick: false,
  });
}

function getPluginLabel(pluginMap: Map<string, PythonPlugin>, pluginId: string) {
  return pluginMap.get(pluginId)?.name ?? pluginId;
}
