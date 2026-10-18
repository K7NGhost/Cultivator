import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import type { CaseRecord } from "@/features/cases";
import { listDataSources, notifyDataSourcesChanged } from "@/features/datasources";
import { listPluginJobs, listPythonPlugins } from "../services/pluginRepository";
import type { PluginRunSnapshot } from "../model/types";
import { restorePluginJobToasts, waitForPluginProgressListener } from "../utils/pluginToasts";

export function usePluginJobMonitor(activeCase: CaseRecord | null) {
  useEffect(() => {
    void waitForPluginProgressListener().catch(console.error);
    const listener = listen<PluginRunSnapshot>("plugin-run-finished", ({ payload }) => {
      if (payload.summary.datasourcePathsUpdated) {
        const caseId = payload.summary.jobs[0]?.caseId;
        if (caseId) notifyDataSourcesChanged(caseId);
      }
    });
    return () => { void listener.then((unlisten) => unlisten()); };
  }, []);

  useEffect(() => {
    if (!activeCase) return;
    const { databasePath } = activeCase;
    let disposed = false;
    const labels = Promise.all([
      listDataSources(activeCase.databasePath, activeCase.id),
      listPythonPlugins(),
    ]);
    async function synchronize() {
      const [jobs, [datasources, plugins]] = await Promise.all([
        listPluginJobs(databasePath, { limit: 500 }), labels,
      ]);
      if (!disposed) restorePluginJobToasts(jobs,
          new Map(datasources.map((item) => [item.id, item.name])),
          new Map(plugins.map((item) => [item.id, item.name])));
    }
    void synchronize().catch(console.error);
    const timer = window.setInterval(() => { void synchronize().catch(console.error); }, 5000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [activeCase?.id, activeCase?.databasePath]);
}
