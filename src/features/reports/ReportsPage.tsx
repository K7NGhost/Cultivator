import { useEffect, useState, type ReactNode } from "react";
import { useLocation } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Archive, FileSpreadsheet, FileText, FolderOpen, RefreshCw } from "lucide-react";
import { toast } from "react-toastify";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useCases } from "@/features/cases/case-provider";
import { collectReportData, type ReportData, type ReportSections } from "@/features/reports/reportData";
import { buildReportHtml, buildTaggedItemsCsv, formatDate } from "@/features/reports/reportHtml";
import { cn } from "@/lib/utils";

type SavedReport = { path: string; sha256: string; attachmentsCopied: number };

const SECTION_LABELS: Array<{ key: keyof ReportSections; label: string; hint: string }> = [
  { key: "tagged", label: "Tagged items", hint: "Every tagged file and photo, with notes and hashes" },
  { key: "photos", label: "Copies of tagged photos", hint: "Shown in the report, saved in a folder beside it" },
  { key: "hashSetHits", label: "Hash set hits", hint: "Files matching imported known-bad or known-good lists" },
  { key: "mismatches", label: "Extension mismatches", hint: "Files whose content contradicts their extension" },
  { key: "processingLog", label: "Processing log", hint: "Plugins run, when, and the AI models and devices used" },
];

/** Builds and exports the case report. */
export function ReportsPage() {
  const { activeCase } = useCases();
  const location = useLocation();
  const isActive = location.pathname === "/reports";
  const [data, setData] = useState<ReportData | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [sections, setSections] = useState<ReportSections>({
    tagged: true,
    photos: true,
    hashSetHits: true,
    mismatches: true,
    processingLog: true,
  });
  const [notes, setNotes] = useState("");
  const [lastSaved, setLastSaved] = useState<SavedReport | null>(null);
  const notesKey = activeCase ? `cultivator.reportNotes.${activeCase.id}` : null;

  // The notes draft is kept per case on this computer.
  useEffect(() => {
    setNotes(notesKey ? (localStorage.getItem(notesKey) ?? "") : "");
    setLastSaved(null);
  }, [notesKey]);

  useEffect(() => {
    if (!activeCase || !isActive) {
      if (!activeCase) {
        setData(null);
      }
      return;
    }
    let isCurrent = true;
    setIsLoading(true);
    collectReportData(activeCase)
      .then((collected) => {
        if (isCurrent) {
          setData(collected);
        }
      })
      .catch((error) => toast.error(errorText(error)))
      .finally(() => {
        if (isCurrent) {
          setIsLoading(false);
        }
      });
    return () => {
      isCurrent = false;
    };
  }, [activeCase, isActive, refreshKey]);

  async function exportReport(format: "html" | "csv") {
    if (!activeCase || !data) {
      return;
    }
    const stamp = new Date().toISOString().slice(0, 10);
    const suggested =
      format === "html" ? `${activeCase.name} report ${stamp}.html` : `${activeCase.name} tagged items ${stamp}.csv`;
    const path = await save({
      defaultPath: suggested.replace(/[\\/:*?"<>|]/g, "_"),
      filters: format === "html" ? [{ name: "HTML report", extensions: ["html"] }] : [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!path) {
      return;
    }

    setIsSaving(true);
    try {
      // Refresh first so the export matches the case right now.
      const current = { ...(await collectReportData(activeCase)), generatedAt: new Date() };
      setData(current);
      const stem = (path.split(/[\\/]/).pop() ?? "report").replace(/\.[^.]+$/, "");
      const { html, attachments } = buildReportHtml(current, sections, notes, `${stem}_files`);
      const saved = await invoke<SavedReport>("save_report", {
        path,
        contents: format === "html" ? html : buildTaggedItemsCsv(current),
        attachments: format === "html" ? attachments : [],
      });
      setLastSaved(saved);
      toast.success(`Report saved. SHA-256 ${saved.sha256.slice(0, 16)}…`);
    } catch (error) {
      toast.error(errorText(error));
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <Archive className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <h1 className="text-sm font-semibold">Reports</h1>
        {activeCase && (
          <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
            {activeCase.name}
          </Badge>
        )}
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="ml-auto h-7 rounded-sm px-2 text-xs"
          disabled={!activeCase || isLoading}
          onClick={() => setRefreshKey((key) => key + 1)}
        >
          <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
          Refresh
        </Button>
      </section>

      {!activeCase ? (
        <p className="p-3 text-xs text-muted-foreground">Open a case to build its report.</p>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[20rem_minmax(0,1fr)]">
          <aside className="flex min-h-0 flex-col gap-3 overflow-auto border-r p-3 text-xs">
            <div className="grid gap-1.5">
              <div className="font-medium">Include</div>
              {SECTION_LABELS.map(({ key, label, hint }) => (
                <label key={key} className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    className="mt-0.5"
                    checked={sections[key]}
                    disabled={key === "photos" && !sections.tagged}
                    onChange={(event) => setSections((current) => ({ ...current, [key]: event.target.checked }))}
                  />
                  <span>
                    {label}
                    <span className="block text-[11px] text-muted-foreground">{hint}</span>
                  </span>
                </label>
              ))}
            </div>

            <label className="grid gap-1">
              <span className="font-medium">Examiner notes</span>
              <textarea
                value={notes}
                onChange={(event) => {
                  setNotes(event.target.value);
                  if (notesKey) localStorage.setItem(notesKey, event.target.value);
                }}
                rows={7}
                placeholder="Summary of findings, methods, scope..."
                className="rounded-sm border bg-background p-2 text-xs"
              />
            </label>

            <div className="grid gap-1.5">
              <Button
                type="button"
                size="xs"
                className="h-8 rounded-sm text-xs"
                disabled={!data || isSaving}
                onClick={() => void exportReport("html")}
              >
                <FileText className="size-3.5" />
                {isSaving ? "Saving…" : "Export report (HTML)…"}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="h-8 rounded-sm text-xs"
                disabled={!data || isSaving || data.tagged.length === 0}
                onClick={() => void exportReport("csv")}
              >
                <FileSpreadsheet className="size-3.5" />
                Export tagged items (CSV)&hellip;
              </Button>
              <p className="text-[11px] text-muted-foreground">
                The HTML report opens in any browser and prints to PDF. Tagged photos are copied into a
                folder beside it, so keep the two together.
              </p>
            </div>

            {lastSaved && (
              <div className="grid gap-1 rounded-sm border p-2">
                <div className="font-medium">Saved</div>
                <code className="break-all text-[11px]">{lastSaved.path}</code>
                <div className="text-[11px]">
                  SHA-256 <code className="break-all">{lastSaved.sha256}</code>
                </div>
                {lastSaved.attachmentsCopied > 0 && (
                  <div className="text-[11px] text-muted-foreground">
                    {lastSaved.attachmentsCopied} photo copies saved beside it.
                  </div>
                )}
                <div className="flex gap-1">
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    className="h-6 rounded-sm px-1.5 text-[11px]"
                    onClick={() => void navigator.clipboard.writeText(lastSaved.sha256).then(() => toast.success("SHA-256 copied."))}
                  >
                    Copy SHA-256
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    className="h-6 rounded-sm px-1.5 text-[11px]"
                    onClick={() => void revealItemInDir(lastSaved.path)}
                  >
                    <FolderOpen className="size-3" />
                    Show in folder
                  </Button>
                </div>
              </div>
            )}
          </aside>

          <div className="min-h-0 overflow-auto p-3 text-xs">
            {!data ? (
              <p className="text-muted-foreground">{isLoading ? "Collecting case data…" : "No data yet."}</p>
            ) : (
              <ReportPreview data={data} sections={sections} />
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/** What the exported report will contain. */
function ReportPreview({ data, sections }: { data: ReportData; sections: ReportSections }) {
  const facts: Array<[string, string]> = [
    ["Examiner", data.caseRecord.examiner || "-"],
    ["Reference", data.caseRecord.reference || "-"],
    ["Data sources", data.dataSources.map((source) => source.name).join(", ") || "-"],
    ["Files hashed", data.counts.hashedFiles.toLocaleString()],
    ["Messages / calls / contacts", `${data.counts.messages.toLocaleString()} / ${data.counts.calls.toLocaleString()} / ${data.counts.contacts.toLocaleString()}`],
    ["Photos / videos", `${data.counts.photos.toLocaleString()} / ${data.counts.videos.toLocaleString()}`],
  ];
  return (
    <div className="mx-auto grid max-w-4xl gap-4">
      <div>
        <h2 className="text-base font-semibold">Case report: {data.caseRecord.name}</h2>
        <p className="text-muted-foreground">Preview of what the export will contain.</p>
      </div>
      <dl className="grid grid-cols-[12rem_minmax(0,1fr)] gap-x-3 gap-y-1">
        {facts.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="truncate">{value}</dd>
          </div>
        ))}
      </dl>

      {sections.tagged && (
        <PreviewSection title={`Tagged items (${data.tagged.length})`} empty="Nothing is tagged yet. Tag files in Files, or photos in Media.">
          {data.tagged.slice(0, 200).map(({ tag, hash }) => (
            <tr key={tag.id} className="border-b align-top">
              <td className="py-1 pr-2">
                <Badge variant="secondary" className="h-4 rounded-sm px-1 text-[10px]">
                  {tag.tagName}
                </Badge>
              </td>
              <td className="py-1 pr-2">
                <div className="font-medium">{tag.fileName}</div>
                <div className="break-all text-[11px] text-muted-foreground">{tag.filePath}</div>
                {tag.comment && <div className="text-[11px]">Note: {tag.comment}</div>}
              </td>
              <td className="py-1 font-mono text-[10px] break-all text-muted-foreground">
                {hash ? `SHA-256 ${hash.sha256}` : "Not hashed"}
              </td>
            </tr>
          ))}
        </PreviewSection>
      )}
      {sections.hashSetHits && (
        <PreviewSection
          title={`Hash set hits (${data.hashSetHits.length})`}
          empty={data.hashSetCount === 0 ? "No hash sets are imported (Analysis → Hash sets…)." : "No files match the imported hash sets."}
        >
          {data.hashSetHits.slice(0, 200).map((hit) => (
            <tr key={`${hit.setId}:${hit.filePath}:${hit.hash}`} className="border-b align-top">
              <td className="py-1 pr-2">{hit.setName}</td>
              <td className="py-1 pr-2 break-all">{hit.filePath}</td>
              <td className="py-1 font-mono text-[10px] break-all text-muted-foreground">{hit.hash}</td>
            </tr>
          ))}
        </PreviewSection>
      )}
      {sections.mismatches && (
        <PreviewSection title={`Extension mismatches (${data.mismatches.length})`} empty="No file's content contradicts its extension.">
          {data.mismatches.slice(0, 200).map((file) => (
            <tr key={file.path} className="border-b">
              <td className="py-1 pr-2 font-medium">{file.name}</td>
              <td className="py-1 break-all text-muted-foreground">{file.path}</td>
            </tr>
          ))}
        </PreviewSection>
      )}
      {sections.processingLog && (
        <PreviewSection title={`Processing log (${data.jobs.length} plugin runs)`} empty="No plugins have run.">
          {data.jobs.slice(0, 200).map((job) => (
            <tr key={job.id} className="border-b">
              <td className="py-1 pr-2">{job.pluginId}</td>
              <td className="py-1 pr-2">{job.status}</td>
              <td className="py-1 text-muted-foreground">{formatDate(job.startedAt)}</td>
            </tr>
          ))}
        </PreviewSection>
      )}
    </div>
  );
}

function PreviewSection({ title, empty, children }: { title: string; empty: string; children: ReactNode[] }) {
  return (
    <section className="grid gap-1">
      <h3 className="border-b pb-1 text-sm font-semibold">{title}</h3>
      {children.length === 0 ? (
        <p className="text-muted-foreground">{empty}</p>
      ) : (
        <table className="w-full">
          <tbody>{children}</tbody>
        </table>
      )}
    </section>
  );
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
