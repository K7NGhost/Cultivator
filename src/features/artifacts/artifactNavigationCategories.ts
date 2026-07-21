import { artifactModelsByKind } from "@/features/artifacts/artifactModels";
import type { StoredArtifactRecord } from "@/features/artifacts/types";

export const ARTIFACT_NAVIGATION_CATEGORIES = [
  "Refined Results",
  "Web Related",
  "Communication",
  "Social Networking",
  "Media",
  "Email & Calendar",
  "Documents",
  "Application Usage",
  "Operating System",
  "Encryption & Credentials",
  "Connected Devices",
  "Location & Travel",
  "Custom",
] as const;

export type ArtifactNavigationCategory =
  (typeof ARTIFACT_NAVIGATION_CATEGORIES)[number];

const KIND_CATEGORY_OVERRIDES = new Map<string, ArtifactNavigationCategory>([
  ["email", "Email & Calendar"],
  ["calendar_entry", "Email & Calendar"],
]);

const MODEL_CATEGORY_MAP = new Map<string, ArtifactNavigationCategory>([
  ["accounts", "Operating System"],
  ["applications", "Application Usage"],
  ["browser", "Web Related"],
  ["calendar", "Email & Calendar"],
  ["calls", "Communication"],
  ["communications", "Communication"],
  ["contacts", "Communication"],
  ["credentials", "Encryption & Credentials"],
  ["files", "Documents"],
  ["journeys", "Location & Travel"],
  ["locations", "Location & Travel"],
  ["maps", "Location & Travel"],
  ["media", "Media"],
  ["messages", "Communication"],
  ["networks", "Connected Devices"],
  ["notes", "Documents"],
  ["search", "Refined Results"],
  ["system", "Operating System"],
  ["timeline", "Refined Results"],
]);

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

export function getArtifactNavigationCategory(
  artifact: StoredArtifactRecord,
): ArtifactNavigationCategory {
  const kind = getArtifactKind(artifact);

  if (kind === "custom_table" || kind === "record") {
    return "Custom";
  }

  const model = artifactModelsByKind.get(
    kind as Parameters<typeof artifactModelsByKind.get>[0],
  );

  if (!model) {
    return "Custom";
  }

  return (
    KIND_CATEGORY_OVERRIDES.get(kind) ??
    MODEL_CATEGORY_MAP.get(model.category) ??
    "Custom"
  );
}
