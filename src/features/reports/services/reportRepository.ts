import { invoke } from "@tauri-apps/api/core";
import type { ReportCatalog, ReportLibrary, ReportRecord, ReportRequest } from "../types";

export function reportCatalog(caseDatabasePath: string) { return invoke<ReportCatalog>("case_report_catalog", { caseDatabasePath }); }
export function listReports(caseDatabasePath: string) { return invoke<ReportLibrary>("list_case_reports", { caseDatabasePath }); }
export function generateReport(caseDatabasePath: string, request: ReportRequest) { return invoke<ReportRecord>("generate_case_report", { caseDatabasePath, request }); }
export function openReport(caseDatabasePath: string, reportId: string, revealFolder = false) { return invoke<void>("open_case_report", { caseDatabasePath, reportId, revealFolder }); }
