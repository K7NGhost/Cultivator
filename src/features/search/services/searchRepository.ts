import { invoke } from "@tauri-apps/api/core";

import type {
  SearchMatch,
  SearchMatchDetailsInput,
  SearchRequest,
  SearchResult,
} from "../model/types";

export function searchFiles(request: SearchRequest): Promise<SearchResult> {
  return invoke<SearchResult>("search_files", { request });
}

export function cancelSearch(searchId: string): Promise<boolean> {
  return invoke<boolean>("cancel_search", { searchId });
}

export function readSearchMatchDetails(
  input: SearchMatchDetailsInput,
): Promise<SearchMatch[]> {
  return invoke<SearchMatch[]>("read_search_match_details", { ...input });
}

export function readSearchPreview(
  path: string,
  line: number,
): Promise<[string[], string[]]> {
  return Promise.all([
    invoke<string[]>("read_text_preview", { path, line }),
    invoke<string[]>("read_hex_preview", { path }),
  ]);
}
