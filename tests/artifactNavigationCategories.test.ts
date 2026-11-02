import { describe, expect, test } from "bun:test";

import {
  ARTIFACT_NAVIGATION_CATEGORIES,
  getArtifactNavigationCategory,
} from "../src/features/artifacts/model/artifactNavigationCategories";
import type { StoredArtifactRecord } from "../src/features/artifacts/model/types";

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

  test("keeps unrecognized custom artifacts in Custom", () => {
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

  test.each([
    ["browser", "Web Related"],
    ["messages", "Communication"],
    ["social", "Social Networking"],
    ["media", "Media"],
    ["calendar", "Email & Calendar"],
    ["files", "Documents"],
    ["applications", "Application Usage"],
    ["system", "Operating System"],
    ["credentials", "Encryption & Credentials"],
    ["networks", "Connected Devices"],
    ["locations", "Location & Travel"],
    ["timeline", "Refined Results"],
    ["email_calendar", "Email & Calendar"],
    ["Web Related", "Web Related"],
  ])("routes custom tables with category %s to %s", (category, expected) => {
    expect(getArtifactNavigationCategory(artifact("custom_table", {
      kind: "custom_table", category, table: { name: "Plugin output", columns: [], rows: [] },
    }))).toBe(expected);
  });

  test.each([
    ["Safari Browser - Bookmarks", "Safari Browser", "Web Related"],
    ["History", "Firefox", "Web Related"],
    ["Instagram Messages", "Instagram", "Communication"],
    ["Posts", "Instagram", "Social Networking"],
    ["Gmail Messages", "Gmail", "Email & Calendar"],
    ["Saved Coordinates", "Photos.sqlite", "Location & Travel"],
    ["Bluetooth Connections", "System", "Connected Devices"],
    ["Photos", "Photos.sqlite", "Media"],
    ["Shared Files", "WhatsApp", "Documents"],
    ["Installed Apps", "Device Information", "Application Usage"],
    ["Battery Events", "PowerLog", "Operating System"],
    ["Safari Passwords", "Safari Browser", "Encryption & Credentials"],
    ["Spotlight Searches", "Spotlight", "Refined Results"],
    ["Alarms", "Clock", "Operating System"],
    ["Google Cast Framework - Devices", "Google Home", "Connected Devices"],
    ["App Data", "Network Usage", "Application Usage"],
  ])("routes LEAPP table %s by content and parser metadata", (name, category, expected) => {
    expect(getArtifactNavigationCategory(artifact("custom_table", {
      kind: "custom_table", category: "other", table: { name, columns: [], rows: [] },
      parser: { category, artifact: { name } },
    }))).toBe(expected);
  });

  test("uses group metadata from older plugin output", () => {
    expect(getArtifactNavigationCategory(artifact("custom_table", {
      kind: "custom_table", group: { id: "parser-output", label: "Safari Browser" },
      table: { name: "History", columns: [], rows: [] },
    }))).toBe("Web Related");
  });

  test("does not inspect evidence rows or source filenames and leaves payload unchanged", () => {
    const payload = {
      kind: "custom_table", category: "other",
      table: { name: "Unknown Output", columns: [{ key: "value" }], rows: [{ value: "password email location" }] },
    };
    const record = { ...artifact("custom_table", payload), filePath: "Safari/History.db" };
    const before = JSON.stringify(record);
    expect(getArtifactNavigationCategory(record)).toBe("Custom");
    expect(JSON.stringify(record)).toBe(before);
  });

  test("normalized kinds remain authoritative over conflicting metadata", () => {
    expect(getArtifactNavigationCategory(artifact("email", {
      kind: "email", category: "browser", label: "SMS Messages",
    }))).toBe("Email & Calendar");
  });

  test("ignores malformed optional metadata", () => {
    expect(getArtifactNavigationCategory(artifact("custom_table", {
      kind: "custom_table", category: 123, parser: [], group: null, table: { name: {} },
    }))).toBe("Custom");
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
