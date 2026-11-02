import type { StoredArtifactRecord } from "./types";

export type ArtifactTreeNode = {
  id: string;
  name: string;
  kind: "category" | "group" | "artifact";
  count: number;
  entryCount: number;
  occurrenceCount: number;
  icon?: string;
  artifact?: StoredArtifactRecord;
  artifacts?: StoredArtifactRecord[];
  children?: ArtifactTreeNode[];
};

