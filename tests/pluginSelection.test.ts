import { describe, expect, test } from "bun:test";

import {
  areAllSelected,
  defaultPluginSelection,
  deselectPlugins,
  selectPlugins,
} from "../src/features/plugins/pluginSelection";

const installed = ["hash-calculator", "image-metadata", "media-tagging", "json-chat-export", "semantic-index"];

describe("plugin selection", () => {
  test("pre-selects every installed plugin that isn't switched off for the case", () => {
    expect(defaultPluginSelection(installed, new Set())).toEqual(installed);
    expect(defaultPluginSelection(installed, new Set(["media-tagging", "not-installed"]))).toEqual([
      "hash-calculator",
      "image-metadata",
      "json-chat-export",
      "semantic-index",
    ]);
  });

  test("select all adds only the listed plugins, once each", () => {
    expect(selectPlugins(["hash-calculator"], ["hash-calculator", "media-tagging"])).toEqual([
      "hash-calculator",
      "media-tagging",
    ]);
  });

  test("clear removes only the listed plugins", () => {
    expect(deselectPlugins(installed, ["media-tagging", "semantic-index"])).toEqual([
      "hash-calculator",
      "image-metadata",
      "json-chat-export",
    ]);
  });

  test("knows when a filtered list is fully selected", () => {
    expect(areAllSelected(installed, ["media-tagging"])).toBe(true);
    expect(areAllSelected(["hash-calculator"], ["hash-calculator", "media-tagging"])).toBe(false);
    expect(areAllSelected(installed, [])).toBe(false);
  });
});
