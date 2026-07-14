import { createElement, useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toast, type Id } from "react-toastify";

import type {
  PluginRunSummary,
  PythonPlugin,
} from "@/features/plugins/types";

type ActiveIngestRun = {
  datasourceName: string;
  jobs: ActiveIngestJob[];
  pluginCount: number;
  runId: string;
  status: "running" | "cancelling";
  onCancel?: () => Promise<void>;
};

type ActiveIngestJob = {
  pluginId: string;
  pluginName: string;
  progress?: PluginProgress;
};

type PluginProgress = {
  completed: number;
  etaSeconds?: number;
  message?: string;
  total: number;
};

type PluginProgressEvent = PluginProgress & { pluginId: string; runId: string };

const activeRuns = new Map<string, ActiveIngestRun>();
const listeners = new Set<() => void>();
let ingestToastId: Id | null = null;
let progressUnlisten: Promise<UnlistenFn> | null = null;

export function createPluginRunId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }

  return `plugin-run-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function showPluginRunStartedToast(input: {
  datasourceName: string;
  jobs?: ActiveIngestJob[];
  onCancel?: () => Promise<void>;
  pluginCount: number;
  runId?: string;
}) {
  const runId = input.runId ?? createPluginRunId();
  const jobs =
    input.jobs && input.jobs.length > 0
      ? input.jobs
      : Array.from({ length: input.pluginCount }, (_, index) => ({
          pluginId: `${runId}:${index}`,
          pluginName: `Plugin ${index + 1}`,
        }));

  activeRuns.set(runId, {
    datasourceName: input.datasourceName,
    jobs,
    onCancel: input.onCancel,
    pluginCount: jobs.length,
    runId,
    status: "running",
  });
  ensureProgressListener();
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
    return;
  }

  if (input.toastId) {
    toast.update(input.toastId, {
      render: `Finished plugin run on ${input.datasourceName}`,
      type: failedJobs.length > 0 ? "warning" : "success",
      isLoading: false,
      autoClose: false,
      closeButton: true,
      closeOnClick: false,
    });
    return;
  }

  void completedJobs;
  void failedJobs;
  void runningJobs;
  void input.pluginMap;
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


  const ingestJobs = runs.flatMap((run) =>
    run.jobs.map((job) => ({
      job,
      run,
    })),
  );
  const activeCount = ingestJobs.length;
  const runCount = runs.length;

  return createElement(
    "div",
    { className: "w-[min(22rem,calc(100vw-3rem))] text-xs" },
    createElement(
      "div",
      { className: "flex items-center justify-between gap-3 font-medium" },
      createElement(
        "span",
        null,
        activeCount > 0
          ? `${activeCount} ingest job${activeCount === 1 ? "" : "s"} running`
          : "No ingest jobs running",
      ),
      createElement(
        "span",
        { className: "shrink-0 text-[11px] opacity-80" },
        `${runCount.toLocaleString()} run${runCount === 1 ? "" : "s"}`,
      ),
    ),
    createElement(
      "div",
      { className: "mt-1 text-[11px] opacity-85" },
      "Progress is shown when reported by a plugin.",
    ),
    createElement(
      "div",
      { className: "mt-1 text-[11px] opacity-75" },
      "Closing this toast does not cancel running jobs.",
    ),
    createElement(
      "div",
      { className: "mt-2 grid max-h-48 gap-1.5 overflow-y-auto overflow-x-hidden pr-1" },
      ingestJobs.map(({ job, run }) =>
        createElement(
          "div",
          {
            className:
              "grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-sm border border-white/25 px-2 py-1",
            key: `${run.runId}:${job.pluginId}`,
          },
          createElement(
            "div",
            { className: "min-w-0" },
            createElement(
              "div",
              { className: "truncate font-medium" },
              job.pluginName,
            ),
            createElement(
              "div",
              { className: "text-[11px] opacity-80" },
              run.status === "cancelling"
                ? `${run.datasourceName} - Cancelling`
                : job.progress?.message ?? `${run.datasourceName} - Running`,
            ),
            job.progress &&
              createElement(
                "div",
                { className: "mt-1" },
                createElement(
                  "div",
                  {
                    className: "h-1.5 overflow-hidden rounded-full bg-black/20",
                    role: "progressbar",
                    "aria-valuemax": job.progress.total,
                    "aria-valuemin": 0,
                    "aria-valuenow": job.progress.completed,
                  },
                  createElement("div", {
                    className: "h-full rounded-full bg-white transition-[width]",
                    style: {
                      width: `${(job.progress.completed / job.progress.total) * 100}%`,
                    },
                  }),
                ),
                createElement(
                  "div",
                  { className: "mt-0.5 text-[10px] opacity-70" },
                  `${job.progress.completed.toLocaleString()} / ${job.progress.total.toLocaleString()}`,
                  job.progress.etaSeconds !== undefined
                    ? ` · ${formatDuration(job.progress.etaSeconds)} remaining`
                    : "",
                ),
              ),
          ),
          run.onCancel
            ? createElement(
                "button",
                {
                  className:
                    "h-6 max-w-20 rounded-sm border border-white/30 px-2 text-[11px] disabled:opacity-50",
                  disabled: run.status === "cancelling",
                  onClick: () => cancelIngestRun(run),
                  type: "button",
                },
                run.status === "cancelling" ? "Cancelling" : "Cancel",
              )
            : createElement(
                "span",
                { className: "text-[11px] opacity-70" },
                "No cancel",
              ),
        ),
      ),
    ),
  );
}

function formatDuration(seconds: number) {
  seconds = Math.max(0, Math.ceil(seconds));
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;

  return minutes > 0
    ? `${minutes}m ${remainingSeconds.toString().padStart(2, "0")}s`
    : `${remainingSeconds}s`;
}

function ensureProgressListener() {
  progressUnlisten ??= listen<PluginProgressEvent>("plugin-progress", (event) => {
    const progress = event.payload;
    const run = activeRuns.get(progress.runId);
    if (!run || progress.total <= 0) return;
    activeRuns.set(progress.runId, {
      ...run,
      jobs: run.jobs.map((job) =>
        job.pluginId === progress.pluginId
          ? { ...job, progress: { ...progress, completed: Math.min(progress.completed, progress.total) } }
          : job,
      ),
    });
    notifyIngestToastSubscribers();
  });
}

function cancelIngestRun(run: ActiveIngestRun) {
  activeRuns.set(run.runId, { ...run, status: "cancelling" });
  notifyIngestToastSubscribers();

  void run.onCancel?.().catch(() => {
    activeRuns.set(run.runId, { ...run, status: "running" });
    notifyIngestToastSubscribers();
  });
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
      onClose: () => {
        if (activeRuns.size > 0) {
          ingestToastId = null;
        }
      },
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
