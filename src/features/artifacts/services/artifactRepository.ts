import { invoke } from "@tauri-apps/api/core";

import type { StoredArtifactRecord } from "../model/types";

const catalogRequests = new Map<string, Promise<StoredArtifactRecord[]>>();

export async function listArtifacts(
  caseDatabasePath: string,
): Promise<StoredArtifactRecord[]> {
  let request = catalogRequests.get(caseDatabasePath);
  if (!request) {
    request = invoke<StoredArtifactRecord[]>("list_artifact_summaries", { caseDatabasePath })
      .finally(() => { catalogRequests.delete(caseDatabasePath); });
    catalogRequests.set(caseDatabasePath, request);
  }
  return request;
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
