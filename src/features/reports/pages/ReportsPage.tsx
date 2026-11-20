import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { ExternalLink, FileCode2, FilePlus2, FolderOpen, LoaderCircle, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useCases, type CaseRecord } from "@/features/cases";
import { useTimeZone } from "@/features/settings/timeZoneSettings";
import { formatTimestamp } from "@/lib/timeZone";
import { cn } from "@/lib/utils";
import { GenerateReportDialog } from "../components/GenerateReportDialog";
import { generateReport, listReports, openReport, reportCatalog } from "../services/reportRepository";
import { reportSize, sectionLabel, type ReportCatalog, type ReportRecord, type ReportRequest } from "../types";

const message = (error: unknown) => error instanceof Error ? error.message : String(error);

export function ReportsPage() {
  const { activeCase } = useCases();
  if (!activeCase) return <div className="flex items-center gap-2 border-b p-2 text-xs text-muted-foreground">Open a case to generate reports.
    <Button asChild variant="outline" size="sm" className="h-7 text-xs"><Link to="/case">Open case</Link></Button>
  </div>;
  return <ReportsWorkspace key={activeCase.databasePath} caseRecord={activeCase} />;
}

export function ReportsWorkspace({ caseRecord }: { caseRecord: CaseRecord }) {
  const timeZone = useTimeZone();
  const active = useLocation().pathname === "/reports";
  const [reports, setReports] = useState<ReportRecord[]>([]);
  const [catalog, setCatalog] = useState<ReportCatalog | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [dialog, setDialog] = useState(false);
  const [error, setError] = useState("");
  const [generationError, setGenerationError] = useState("");
  const request = useRef(0);
  const selected = reports.find((report) => report.id === selectedId);
  const refresh = useCallback(async () => {
    const version = ++request.current;
    setLoading(true); setError("");
    try {
      const [library, options] = await Promise.all([listReports(caseRecord.databasePath), reportCatalog(caseRecord.databasePath)]);
      if (version !== request.current) return;
      setReports(library.reports); setWarnings(library.warnings); setCatalog(options);
      setSelectedId((previous) => library.reports.some((report) => report.id === previous) ? previous : library.reports[0]?.id ?? null);
    } catch (error) { if (version === request.current) setError(message(error)); }
    finally { if (version === request.current) setLoading(false); }
  }, [caseRecord.databasePath]);
  useEffect(() => { if (active) void refresh(); return () => { request.current++; }; }, [active, refresh]);

  async function open(report: ReportRecord, folder = false) {
    try { await openReport(caseRecord.databasePath, report.id, folder); }
    catch (error) { setError(message(error)); }
  }
  async function generate(options: ReportRequest, openAfter: boolean) {
    setGenerating(true); setGenerationError("");
    try {
      const report = await generateReport(caseRecord.databasePath, options);
      setReports((current) => [report, ...current]); setSelectedId(report.id); setDialog(false);
      if (openAfter) await open(report);
    } catch (error) { setGenerationError(message(error)); }
    finally { setGenerating(false); }
  }

  return <section className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden" aria-label="Case reports">
    <header className="flex shrink-0 items-center gap-2 border-b px-3 py-2">
      <FileCode2 className="size-4 text-primary" /><h1 className="text-sm font-semibold">Reports</h1>
      <span className="text-xs text-muted-foreground">HTML examination reports</span>
      <span className="ml-auto text-xs text-muted-foreground">{reports.length} saved</span>
      <Button variant="outline" size="sm" className="h-7 text-xs" disabled={loading || generating} onClick={() => void refresh()}>
        {loading ? <LoaderCircle className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}Refresh
      </Button>
      <Button size="sm" className="h-7 text-xs" disabled={!catalog || loading || generating} onClick={() => { setGenerationError(""); setDialog(true); }}><FilePlus2 className="size-3.5" />Generate report</Button>
    </header>
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b bg-muted/15 px-3 py-1.5 text-[11px] text-muted-foreground">
      <span>Case: {caseRecord.name}</span><span className="ml-auto">Saved with the case · Opens offline · Print-ready HTML</span>
    </div>
    {error && <p role="alert" className="shrink-0 border-b px-3 py-2 text-xs text-destructive">{error}</p>}
    {warnings.map((warning) => <p role="status" key={warning} className="shrink-0 border-b px-3 py-1 text-xs text-muted-foreground">{warning}</p>)}
    <div className="flex min-h-0 flex-1">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto" aria-busy={loading}>
        {reports.length ? <table className="w-full border-collapse text-left text-xs">
          <thead className="sticky top-0 bg-background text-[11px] text-muted-foreground"><tr>{["Report", "Generated", "Prepared by", "Sources", "Size"].map((column) => <th className="border-b px-3 py-2 font-medium" key={column}>{column}</th>)}</tr></thead>
          <tbody>{reports.map((report) => <tr key={report.id} className={cn("cursor-pointer border-b hover:bg-muted/40", selectedId === report.id && "bg-primary/10")} onClick={() => setSelectedId(report.id)}>
            <td className="px-3 py-2"><button type="button" className="text-left font-medium hover:underline" aria-pressed={selectedId === report.id} onClick={() => setSelectedId(report.id)}>{report.title}</button><span className="mt-0.5 block text-[11px] text-muted-foreground">{report.sections.length} sections · HTML</span></td>
            <td className="whitespace-nowrap px-3 py-2 text-[11px]">{formatTimestamp(report.createdAt, timeZone)}</td>
            <td className="px-3 py-2">{report.examiner || "—"}</td><td className="px-3 py-2">{report.datasourceIds.length}</td><td className="whitespace-nowrap px-3 py-2 text-muted-foreground">{reportSize(report.sizeBytes)}</td>
          </tr>)}</tbody>
        </table> : <div role="status" className="grid flex-1 place-items-center p-4 text-center text-xs text-muted-foreground"><div>
          {loading ? <p className="flex items-center gap-2"><LoaderCircle className="size-4 animate-spin" />Loading reports…</p> : <><FileCode2 className="mx-auto mb-2 size-6" /><p className="font-medium text-foreground">No reports generated yet</p><p className="mt-1">Choose Generate report to select sections and datasources.</p></>}
        </div></div>}
      </div>
      <aside className="flex w-80 shrink-0 flex-col overflow-y-auto border-l" aria-label="Report details">
        <h2 className="shrink-0 border-b px-3 py-2 text-xs font-semibold">Report details</h2>
        {selected ? <>
          <div className="space-y-2 border-b p-3"><h3 className="break-words text-sm font-semibold">{selected.title}</h3><div className="flex gap-1">
            <Button variant="outline" size="sm" className="h-7 text-xs" onClick={() => void open(selected)}><ExternalLink className="size-3.5" />Open report</Button>
            <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => void open(selected, true)}><FolderOpen className="size-3.5" />Show folder</Button>
          </div></div>
          <dl className="divide-y px-3 text-xs">{[["Generated", formatTimestamp(selected.createdAt, timeZone)], ["Prepared by", selected.examiner || "—"], ["Format", "Self-contained HTML"], ["File size", reportSize(selected.sizeBytes)], ["Report ID", selected.id], ["File", selected.path], ["SHA-256 at generation", selected.sha256]].map(([label, value]) => <div key={label} className="space-y-1 py-2"><dt className="text-[11px] text-muted-foreground">{label}</dt><dd className="select-text break-all font-mono text-[11px]">{value}</dd></div>)}</dl>
          <div className="border-t p-3"><h3 className="mb-2 text-xs font-semibold">Included sections</h3><ul className="space-y-2 text-xs">{selected.counts.map((count) => <li key={count.section} className="flex justify-between gap-2"><span>{sectionLabel(count.section)}</span><span className="text-muted-foreground">{count.rows.toLocaleString()} rows</span></li>)}</ul></div>
          <p className="border-t p-3 text-[11px] text-muted-foreground">This saved report is a snapshot. Generate a new report to include later case changes.</p>
        </> : <p className="p-3 text-xs text-muted-foreground">Select a report to view its contents and open the HTML file.</p>}
      </aside>
    </div>
    <footer className="flex h-6 shrink-0 items-center border-t px-3 text-[10px] text-muted-foreground">{generating ? "Generating selected report sections…" : `${reports.length} HTML reports in this case`}</footer>
    {dialog && catalog && <GenerateReportDialog caseRecord={caseRecord} catalog={catalog} busy={generating} error={generationError} onClose={() => setDialog(false)} onGenerate={generate} />}
  </section>;
}
