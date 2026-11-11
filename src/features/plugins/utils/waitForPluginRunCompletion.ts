import type { PluginRunSnapshot, PluginRunSummary } from "../model/types";

export async function waitForPluginRunCompletion(
  readSnapshot: () => Promise<PluginRunSnapshot>,
  wait: () => Promise<void> = () =>
    new Promise((resolve) => globalThis.setTimeout(resolve, 1000)),
): Promise<PluginRunSummary> {
  while (true) {
    try {
      const snapshot = await readSnapshot();
      if (snapshot.finished) return snapshot.summary;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      // A busy status read does not mean the backend job failed. Retry only the
      // snapshot; resubmitting a run could start the same extraction twice.
      if (!/\bdatabase is locked\b|\bSQLITE_BUSY(?:_[A-Z]+)?\b/i.test(message)) {
        throw error;
      }
    }
    await wait();
  }
}
