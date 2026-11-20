export type ReportSection = "caseInformation" | "evidenceSources" | "artifacts" | "media" | "timeline" | "processingLog";
export type ReportRequest = { title: string; examiner: string; notes: string; sections: ReportSection[]; datasourceIds: string[] };
export type SectionCount = { section: ReportSection; rows: number };
export type ReportRecord = {
  id: string; title: string; examiner: string; createdAt: string; sections: ReportSection[];
  datasourceIds: string[]; counts: SectionCount[]; sizeBytes: number; sha256: string; path: string;
};
export type ReportDatasource = { id: string; name: string; sourceType: string; path: string };
export type ReportCatalog = { datasources: ReportDatasource[]; sections: SectionCount[] };
export type ReportLibrary = { reports: ReportRecord[]; warnings: string[] };

export const REPORT_SECTIONS: ReadonlyArray<{ id: ReportSection; label: string; description: string; unit: string }> = [
  { id: "caseInformation", label: "Case information", description: "Case name, reference number, examiner and description.", unit: "case" },
  { id: "evidenceSources", label: "Evidence sources", description: "Selected datasources, imported paths and original evidence references.", unit: "sources" },
  { id: "artifacts", label: "Artifact records", description: "Readable artifact tables from the latest completed plugin runs, including every saved row.", unit: "artifact groups" },
  { id: "media", label: "Media inventory and stored hashes", description: "Image/video metadata, stored hashes, manual review and analysis results. Media files are not embedded.", unit: "files" },
  { id: "timeline", label: "Timeline events", description: "Chronological events derived from saved artifact timestamps. Built when generated if needed.", unit: "indexed events" },
  { id: "processingLog", label: "Processing history and logs", description: "Plugin runs, completion states, errors and recorded log messages.", unit: "runs" },
];

export function sectionLabel(section: ReportSection) { return REPORT_SECTIONS.find((item) => item.id === section)?.label ?? section; }
export function reportSize(bytes: number) { return bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`; }
