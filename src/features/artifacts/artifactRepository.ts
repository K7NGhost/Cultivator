import { invoke } from "@tauri-apps/api/core";

import type { StoredArtifactRecord } from "@/features/artifacts/types";

export async function listArtifacts(
  caseDatabasePath: string,
): Promise<StoredArtifactRecord[]> {
  return invoke<StoredArtifactRecord[]>("list_plugin_artifacts", {
    caseDatabasePath,
  });
}

export async function deleteArtifact(
  caseDatabasePath: string,
  artifactId: string,
): Promise<void> {
  return invoke("delete_plugin_artifact", {
    caseDatabasePath,
    artifactId,
  });
}

export async function deleteArtifacts(
  caseDatabasePath: string,
  artifactIds: string[],
): Promise<void> {
  return invoke("delete_plugin_artifacts", {
    caseDatabasePath,
    artifactIds,
  });
}
