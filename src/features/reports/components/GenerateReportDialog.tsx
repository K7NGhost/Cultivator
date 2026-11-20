import { useState } from "react";
import { FileCode2, LoaderCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { CaseRecord } from "@/features/cases";
import { REPORT_SECTIONS, type ReportCatalog, type ReportRequest, type ReportSection } from "../types";

export function GenerateReportDialog({ caseRecord, catalog, busy, error, onClose, onGenerate }: {
  caseRecord: CaseRecord; catalog: ReportCatalog; busy: boolean; error: string;
  onClose: () => void; onGenerate: (request: ReportRequest, openAfter: boolean) => Promise<void>;
}) {
  const [title, setTitle] = useState(`${caseRecord.name} — Examination report`);
  const [examiner, setExaminer] = useState(caseRecord.examiner);
  const [notes, setNotes] = useState("");
  const [sections, setSections] = useState<ReportSection[]>(catalog.datasources.length ? ["caseInformation", "evidenceSources", "artifacts", "media", "processingLog"] : ["caseInformation"]);
  const [sources, setSources] = useState(catalog.datasources.map((source) => source.id));
  const [openAfter, setOpenAfter] = useState(true);
  const needsSources = catalog.datasources.length > 0 && sections.some((section) => section !== "caseInformation");
  const valid = !!title.trim() && sections.length > 0 && (!needsSources || sources.length > 0);

  return <Dialog open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
    <DialogContent className="flex max-h-[90vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-3xl" showCloseButton={!busy}
      onInteractOutside={(event) => { if (busy) event.preventDefault(); }} onEscapeKeyDown={(event) => { if (busy) event.preventDefault(); }}>
      <DialogHeader className="shrink-0 border-b px-4 py-3">
        <DialogTitle className="flex items-center gap-2 text-sm"><FileCode2 className="size-4" />Generate HTML report</DialogTitle>
        <DialogDescription className="text-xs">Choose the saved case data to include. The report opens offline in a browser and can be printed.</DialogDescription>
      </DialogHeader>
      <form className="flex min-h-0 flex-1 flex-col" onSubmit={(event) => {
        event.preventDefault();
        if (valid && !busy) void onGenerate({ title: title.trim(), examiner: examiner.trim(), notes, sections, datasourceIds: sources }, openAfter);
      }}>
        <fieldset disabled={busy} className="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 py-3 text-xs">
          <div className="grid grid-cols-[minmax(0,2fr)_minmax(0,1fr)] gap-3">
            <label className="space-y-1">Report title<Input className="h-8 text-xs" aria-label="Report title" value={title} onChange={(event) => setTitle(event.target.value)} maxLength={200} required /></label>
            <label className="space-y-1">Prepared by<Input className="h-8 text-xs" aria-label="Report examiner" value={examiner} onChange={(event) => setExaminer(event.target.value)} maxLength={200} /></label>
          </div>
          <section aria-label="Report sections" className="rounded-sm border">
            <div className="flex items-center justify-between border-b bg-muted/30 px-2 py-1"><h3 className="font-semibold">Include in report</h3><div className="flex gap-1">
              <Button type="button" variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={() => setSections(REPORT_SECTIONS.map((section) => section.id))}>All</Button>
              <Button type="button" variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={() => setSections([])}>None</Button>
            </div></div>
            <div className="divide-y">{REPORT_SECTIONS.map((section) => {
              const count = catalog.sections.find((item) => item.section === section.id)?.rows ?? 0;
              return <label key={section.id} className="flex cursor-pointer items-start gap-2 px-3 py-2">
                <Checkbox className="mt-0.5" checked={sections.includes(section.id)} aria-label={`Include ${section.label}`} onCheckedChange={(checked) => setSections((current) => checked ? [...current, section.id] : current.filter((id) => id !== section.id))} />
                <span className="min-w-0 flex-1"><span className="font-medium">{section.label}</span><span className="mt-0.5 block text-[11px] text-muted-foreground">{section.description}</span></span>
                <span className="shrink-0 text-[11px] text-muted-foreground">{count.toLocaleString()} {section.unit}</span>
              </label>;
            })}</div>
          </section>
          <section aria-label="Report datasources" className="rounded-sm border">
            <div className="flex items-center gap-2 border-b bg-muted/30 px-3 py-2">
              <Checkbox aria-label="Select all report datasources" disabled={!catalog.datasources.length} checked={sources.length === catalog.datasources.length && sources.length > 0 ? true : sources.length ? "indeterminate" : false}
                onCheckedChange={(checked) => setSources(checked ? catalog.datasources.map((source) => source.id) : [])} />
              <h3 className="font-semibold">Datasources</h3><span className="ml-auto text-[11px] text-muted-foreground">{sources.length} selected</span>
            </div>
            <div className="max-h-36 divide-y overflow-y-auto">{catalog.datasources.map((source) => <label key={source.id} className="flex cursor-pointer items-start gap-2 px-3 py-2">
              <Checkbox className="mt-0.5" checked={sources.includes(source.id)} aria-label={`Include datasource ${source.name}`} onCheckedChange={(checked) => setSources((current) => checked ? [...current, source.id] : current.filter((id) => id !== source.id))} />
              <span className="min-w-0"><span className="font-medium">{source.name}</span><span className="block truncate text-[11px] text-muted-foreground" title={source.path}>{source.path}</span></span>
            </label>)}{!catalog.datasources.length && <p className="px-3 py-2 text-muted-foreground">No datasources are saved in this case. Case information can still be reported.</p>}</div>
          </section>
          <label className="block space-y-1">Examiner notes<textarea className="textarea block min-h-20 w-full rounded-sm border-border bg-background p-2 text-xs" aria-label="Report notes" placeholder="Scope, observations, or examination notes to include…" value={notes} onChange={(event) => setNotes(event.target.value)} maxLength={20000} /></label>
          <label className="flex cursor-pointer items-center gap-2"><Checkbox checked={openAfter} onCheckedChange={(checked) => setOpenAfter(checked === true)} />Open report in browser when finished</label>
          <p className="text-[11px] text-muted-foreground">Saved in this case’s reports folder. Original evidence files are referenced, not copied. Stored hashes are included as recorded; report generation does not verify source evidence.</p>
        </fieldset>
        <div className="shrink-0 border-t px-4 py-2">
          {error && <p role="alert" className="mb-2 text-xs text-destructive">{error}</p>}
          {!valid && <p className="mb-2 text-xs text-muted-foreground">{!title.trim() ? "Enter a report title." : !sections.length ? "Select at least one report section." : "Select a datasource for the chosen evidence sections."}</p>}
          <DialogFooter className="gap-2">
            <Button type="button" variant="outline" size="sm" className="h-8 text-xs" disabled={busy} onClick={onClose}>Cancel</Button>
            <Button type="submit" size="sm" className="h-8 text-xs" disabled={!valid || busy}>{busy ? <LoaderCircle className="size-3.5 animate-spin" /> : <FileCode2 className="size-3.5" />}{busy ? "Generating report…" : "Generate report"}</Button>
          </DialogFooter>
        </div>
      </form>
    </DialogContent>
  </Dialog>;
}
