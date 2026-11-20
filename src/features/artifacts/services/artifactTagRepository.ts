import { invoke } from "@tauri-apps/api/core";

export type ArtifactRowTag = { name: string; occurrences: number };
export type ArtifactRowTags = { rows: Record<string, ArtifactRowTag[]>; availableTags: string[] };
export type TaggedArtifactSummary = { artifactId: string; entryCount: number };
const changeEvent = "cultivator:artifact-row-tags-changed";

export function subscribeArtifactTagChanges(databasePath: string, listener: () => void) {
  const handle = (event: Event) => {
    if ((event as CustomEvent<{ databasePath: string }>).detail.databasePath === databasePath) listener();
  };
  window.addEventListener(changeEvent, handle);
  return () => window.removeEventListener(changeEvent, handle);
}

export function loadTaggedArtifactCatalog(caseDatabasePath: string) {
  return invoke<TaggedArtifactSummary[]>("tagged_artifact_catalog", { caseDatabasePath });
}

export function loadArtifactRowTags(caseDatabasePath: string, viewKey: string, rowKeys: string[]) {
  return invoke<ArtifactRowTags>("artifact_row_tags", { caseDatabasePath, viewKey, rowKeys });
}

export async function setArtifactRowTag(caseDatabasePath: string, viewKey: string, rowKey: string, tagName: string, assigned: boolean) {
  const result = await invoke<ArtifactRowTags>("set_artifact_row_tag", { caseDatabasePath, viewKey, rowKey, tagName, assigned });
  window.dispatchEvent(new CustomEvent(changeEvent, { detail: { databasePath: caseDatabasePath } }));
  return result;
}
