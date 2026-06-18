import { invoke } from "@tauri-apps/api/core";

import type { StoredArtifactRecord } from "@/features/artifacts/types";

export async function listArtifacts(
  caseDatabasePath: string,
): Promise<StoredArtifactRecord[]> {
  return invoke<StoredArtifactRecord[]>("list_plugin_artifacts", {
    caseDatabasePath,
  });
}
