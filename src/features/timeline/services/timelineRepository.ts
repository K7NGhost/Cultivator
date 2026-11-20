import { invoke } from "@tauri-apps/api/core";

export type TimelineSummary = { events: number; artifacts: number; datedArtifacts: number; undated: number };
export type TimelineFilter = { search: string; datasourceId: string; label: string; basis: string; from: string; to: string; descending: boolean; rangeFrom?: number; rangeTo?: number };
export type TimelineRange = { from: number; to: number };
export type TimelineBucket = { start: number; end: number; count: number };
export type TimelineOverview = { total: number; buckets: TimelineBucket[] };
export type TimelineEvent = {
  id: number; artifactId: string; rowIndex: number; fieldKey: string; eventType: string;
  displayTime: string; timeBasis: "utc" | "unspecified" | "date"; label: string; summary: string;
  datasourceId: string; pluginId: string; filePath: string;
};
export type TimelinePageData = { items: TimelineEvent[]; total: number; labels: string[] };
export type TimelineDetail = { event: TimelineEvent; rawTime: unknown; row: unknown; parser: unknown; source: unknown; sources: unknown; provenance: unknown; kind: string };

export function syncTimeline(caseDatabasePath: string) {
  return invoke<TimelineSummary>("sync_artifact_timeline", { caseDatabasePath });
}
export function timelinePage(caseDatabasePath: string, filter: TimelineFilter, offset: number, limit: number) {
  return invoke<TimelinePageData>("artifact_timeline_page", { caseDatabasePath, filter, offset, limit });
}
export function timelineDetail(caseDatabasePath: string, eventId: number) {
  return invoke<TimelineDetail>("artifact_timeline_detail", { caseDatabasePath, eventId });
}
export function timelineOverview(caseDatabasePath: string, filter: TimelineFilter) {
  return invoke<TimelineOverview>("artifact_timeline_overview", { caseDatabasePath, filter });
}
