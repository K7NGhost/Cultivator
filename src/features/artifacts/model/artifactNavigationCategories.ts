import { artifactCategoryByKind, artifactCategoryDefinitions, type ArtifactNavigationCategory } from "./artifactCategoryDefinitions";
import type { StoredArtifactRecord } from "./types";

export type { ArtifactNavigationCategory } from "./artifactCategoryDefinitions";
export const ARTIFACT_NAVIGATION_CATEGORIES = artifactCategoryDefinitions.map((category) => category.label);

function getArtifactKind(artifact: StoredArtifactRecord) {
  const payload = artifact.payload;

  if (
    payload &&
    typeof payload === "object" &&
    !Array.isArray(payload) &&
    typeof payload.kind === "string"
  ) {
    return payload.kind;
  }

  return artifact.resultKind;
}

export function getArtifactNavigationCategory(artifact: StoredArtifactRecord): ArtifactNavigationCategory {
  return artifactCategoryByKind.get(getArtifactKind(artifact))?.label ?? "Custom";
}
