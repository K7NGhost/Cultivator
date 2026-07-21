import { describe, expect, test } from "bun:test";

import {
  ARTIFACT_NAVIGATION_CATEGORIES,
  getArtifactNavigationCategory,
} from "../src/features/artifacts/artifactNavigationCategories";
import type { StoredArtifactRecord } from "../src/features/artifacts/types";

function artifact(
  resultKind: string,
  payload: Record<string, unknown>,
): StoredArtifactRecord {
  return {
    id: `${resultKind}-1`,
    jobId: "job-1",
    pluginId: "test-plugin",
    datasourceId: "datasource-1",
    filePath: "evidence.db",
    resultKind,
    label: resultKind,
    payload,
    createdAt: "2026-07-21T00:00:00Z",
  };
}

describe("artifact navigation categories", () => {
  test("uses the requested fixed category order", () => {
    expect(ARTIFACT_NAVIGATION_CATEGORIES).toEqual([
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
    ]);
  });

  test("routes every custom table to Custom regardless of its payload category", () => {
    expect(
      getArtifactNavigationCategory(
        artifact("custom_table", {
          kind: "custom_table",
          category: "vehicles",
          table: { columns: [], name: "Vehicle Speed", rows: [] },
        }),
      ),
    ).toBe("Custom");
  });

  test("maps normalized artifact kinds into the reference taxonomy", () => {
    expect(
      getArtifactNavigationCategory(
        artifact("browser_history", {
          kind: "browser_history",
          category: "browser",
        }),
      ),
    ).toBe("Web Related");
    expect(
      getArtifactNavigationCategory(
        artifact("email", { kind: "email", category: "communications" }),
      ),
    ).toBe("Email & Calendar");
    expect(
      getArtifactNavigationCategory(
        artifact("bluetooth_device", {
          kind: "bluetooth_device",
          category: "networks",
        }),
      ),
    ).toBe("Connected Devices");
  });

  test("routes unknown plugin-defined records to Custom", () => {
    expect(
      getArtifactNavigationCategory(
        artifact("vehicle_speed", {
          kind: "vehicle_speed",
          category: "vehicles",
        }),
      ),
    ).toBe("Custom");
  });
});
