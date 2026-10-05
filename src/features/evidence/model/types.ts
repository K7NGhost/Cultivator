export type EvidenceEntryKind = "datasource" | "directory" | "file";

export type EvidenceTreeNode = {
  id: string;
  name: string;
  path: string;
  kind: EvidenceEntryKind;
  files: number;
  size?: number;
  modifiedMs?: number;
  childCount?: number;
  children?: EvidenceTreeNode[];
};

export type EvidenceDirectoryEntry = {
  id: string;
  name: string;
  path: string;
  kind: EvidenceEntryKind;
  size?: number;
  modifiedMs?: number;
  childCount?: number;
};

export type EvidenceDirectoryListing = {
  rootPath: string;
  rootName: string;
  tree: EvidenceTreeNode;
  entries: EvidenceDirectoryEntry[];
};

