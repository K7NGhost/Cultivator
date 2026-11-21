import type { IndexingStage } from "./indexingSettings";

export function indexingProgress(stage: IndexingStage) {
  const saved = ["Complete", "Checking", "Check failed"].includes(stage.state);
  const known = stage.total > 0 || saved;
  const percent = stage.total ? Math.min(100, Math.round(stage.processed / stage.total * 100)) : saved ? 100 : 0;
  const checking = stage.state === "Checking" || stage.state === "Check failed";
  const summary = `${stage.state}${checking ? ` · ${(stage.checked ?? 0).toLocaleString()} checked · saved coverage` : ""} · ${stage.processed.toLocaleString()}${known ? ` / ${stage.total.toLocaleString()} (${percent}%)` : " discovered"}${stage.skipped > 0 ? ` · ${stage.skipped.toLocaleString()} excluded` : ""}`;
  return { summary, value: known || !["Discovering", "Indexing"].includes(stage.state) ? percent : undefined };
}
