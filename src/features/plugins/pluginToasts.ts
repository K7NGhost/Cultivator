import { createElement, useEffect, useState } from "react";
import { toast, type Id } from "react-toastify";

import type {
  PluginRunSummary,
  PythonPlugin,
} from "@/features/plugins/types";

type ActiveIngestRun = {
  datasourceName: string;
  pluginCount: number;
  runId: string;
  status: "running" | "cancelling";
  onCancel?: () => Promise<void>;
};

const activeRuns = new Map<string, ActiveIngestRun>();
const listeners = new Set<() => void>();
let ingestToastId: Id | null = null;

export function createPluginRunId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }

  return `plugin-run-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function showPluginRunStartedToast(input: {
  datasourceName: string;
  onCancel?: () => Promise<void>;
  pluginCount: number;
  runId?: string;
}) {
  const runId = input.runId ?? createPluginRunId();

  activeRuns.set(runId, {
    datasourceName: input.datasourceName,
    onCancel: input.onCancel,
    pluginCount: input.pluginCount,
    runId,
    status: "running",
  });
  renderIngestToast();

  return ingestToastId ?? runId;
}

export function showPluginRunFinishedToasts(input: {
  datasourceName: string;
  pluginMap: Map<string, PythonPlugin>;
  runId?: string;
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

  if (input.runId) {
    activeRuns.delete(input.runId);
    renderIngestToast();
  } else if (input.toastId) {
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
  runId?: string;
  toastId?: Id;
}) {
  const message =
    input.error instanceof Error ? input.error.message : String(input.error);

  if (input.runId) {
    activeRuns.delete(input.runId);
    renderIngestToast();
  } else if (input.toastId) {
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

function IngestRunsToast() {
  const [isExpanded, setIsExpanded] = useState(false);
  const [runs, setRuns] = useState(() => Array.from(activeRuns.values()));

  useEffect(() => {
    function updateRuns() {
      setRuns(Array.from(activeRuns.values()));
    }

    listeners.add(updateRuns);

    return () => {
      listeners.delete(updateRuns);
    };
  }, []);

  const activeCount = runs.length;
  const pluginCount = runs.reduce((count, run) => count + run.pluginCount, 0);

  return createElement(
    "div",
    { className: "min-w-72 text-xs" },
    createElement(
      "button",
      {
        className:
          "flex w-full items-center justify-between gap-3 rounded-sm text-left font-medium",
        onClick: () => setIsExpanded((currentValue) => !currentValue),
        type: "button",
      },
      createElement(
        "span",
        null,
        activeCount > 0
          ? `${activeCount} ingest module${activeCount === 1 ? "" : "s"} running`
          : "No ingest modules running",
      ),
      createElement("span", { className: "text-[11px] opacity-80" }, isExpanded ? "Hide" : "Show"),
    ),
    createElement(
      "div",
      { className: "mt-1 text-[11px] opacity-85" },
      `${pluginCount.toLocaleString()} plugin${pluginCount === 1 ? "" : "s"} active`,
    ),
    isExpanded
      ? createElement(
          "div",
          { className: "mt-2 grid gap-1.5" },
          runs.map((run) =>
            createElement(
              "div",
              {
                className:
                  "grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-sm border border-white/25 px-2 py-1",
                key: run.runId,
              },
              createElement(
                "div",
                { className: "min-w-0" },
                createElement(
                  "div",
                  { className: "truncate font-medium" },
                  run.datasourceName,
                ),
                createElement(
                  "div",
                  { className: "text-[11px] opacity-80" },
                  `${run.pluginCount.toLocaleString()} plugin${
                    run.pluginCount === 1 ? "" : "s"
                  } - ${run.status === "cancelling" ? "Cancelling" : "Running"}`,
                ),
              ),
              createElement(
                "button",
                {
                  className:
                    "h-6 rounded-sm border border-white/30 px-2 text-[11px] disabled:opacity-50",
                  disabled: run.status === "cancelling" || !run.onCancel,
                  onClick: () => {
                    activeRuns.set(run.runId, { ...run, status: "cancelling" });
                    notifyIngestToastSubscribers();
                    void run.onCancel?.().catch(() => {
                      activeRuns.set(run.runId, { ...run, status: "running" });
                      notifyIngestToastSubscribers();
                    });
                  },
                  type: "button",
                },
                run.status === "cancelling" ? "Cancelling" : "Cancel",
              ),
            ),
          ),
        )
      : null,
  );
}

function renderIngestToast() {
  notifyIngestToastSubscribers();

  if (activeRuns.size === 0) {
    if (ingestToastId) {
      const completedToastId = ingestToastId;

      toast.update(ingestToastId, {
        render: "All ingest modules finished",
        type: "success",
        isLoading: false,
        autoClose: 2500,
        closeButton: true,
        closeOnClick: false,
      });
      window.setTimeout(() => {
        if (ingestToastId === completedToastId && activeRuns.size === 0) {
          ingestToastId = null;
        }
      }, 2500);
    }

    return;
  }

  if (!ingestToastId) {
    ingestToastId = toast.loading(createElement(IngestRunsToast), {
      closeButton: true,
      closeOnClick: false,
    });
    return;
  }

  toast.update(ingestToastId, {
    render: createElement(IngestRunsToast),
    isLoading: true,
    autoClose: false,
    closeButton: true,
    closeOnClick: false,
  });
}

function notifyIngestToastSubscribers() {
  for (const listener of listeners) {
    listener();
  }
}

function getPluginLabel(pluginMap: Map<string, PythonPlugin>, pluginId: string) {
  return pluginMap.get(pluginId)?.name ?? pluginId;
}
