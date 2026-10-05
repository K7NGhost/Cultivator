export type SearchOptions = {
  query: string;
  regex: boolean;
  caseSensitive: boolean;
  binaryFiles: boolean;
};

export type SearchRequest = SearchOptions & {
  searchId: string;
  rootPath: string;
};

export type SearchMatchDetailsInput = SearchOptions & {
  path: string;
};

export type SearchMatch = {
  id: string;
  file: string;
  path: string;
  line: number;
  column: number;
  kind: string;
  matchedText: string;
  context: string;
};

export type SearchResult = {
  matches: SearchMatch[];
  elapsedMs: number;
  cancelled: boolean;
  scannedFiles: number;
  totalFiles: number;
  totalComplete: boolean;
};

export type SearchProgress = {
  searchId: string;
  scannedFiles: number;
  totalFiles: number;
  totalComplete: boolean;
  elapsedMs: number;
};

export type SearchFileSummary = {
  path: string;
  file: string;
  kind: string;
  matchCount: number;
  matchedLines: number[];
  firstMatch: SearchMatch;
};

export type SearchSummaries = {
  searchId: string;
  files: SearchFileSummary[];
  elapsedMs: number;
};

export type SearchScope = {
  id: string;
  name: string;
  description: string;
  paths: string[];
};

export type FileMatch = {
  match: SearchMatch;
  matchCount: number;
  matchedLines: Set<number>;
};
