import { describe, expect, test } from "bun:test";

import { getCustomTableViewportLayout } from "../src/features/artifacts/customTableLayout";

describe("custom table viewport layout", () => {
  test("fills spare horizontal space across data columns", () => {
    const layout = getCustomTableViewportLayout({
      baseColumnWidths: [144, 144],
      fixedTrailingWidth: 0,
      headerHeight: 30,
      rowCount: 2,
      rowHeight: 30,
      scrollbarSize: 16,
      viewportHeight: 400,
      viewportWidth: 800,
    });

    expect(layout.columnWidths).toEqual([400, 400]);
    expect(layout.renderedTableWidth).toBe(800);
    expect(layout.hasHorizontalScrollbar).toBe(false);
    expect(layout.hasVerticalScrollbar).toBe(false);
  });

  test("reserves the vertical scrollbar gutter beside the header", () => {
    const layout = getCustomTableViewportLayout({
      baseColumnWidths: [144, 144],
      fixedTrailingWidth: 136,
      headerHeight: 30,
      rowCount: 100,
      rowHeight: 30,
      scrollbarSize: 16,
      viewportHeight: 400,
      viewportWidth: 800,
    });

    expect(layout.columnWidths).toEqual([324, 324]);
    expect(layout.tableContentWidth).toBe(784);
    expect(layout.scrollbarGutterWidth).toBe(16);
    expect(layout.renderedTableWidth).toBe(800);
    expect(layout.hasVerticalScrollbar).toBe(true);
  });

  test("keeps horizontal scrolling outside the vertically scrolling body", () => {
    const layout = getCustomTableViewportLayout({
      baseColumnWidths: [280, 280, 280],
      fixedTrailingWidth: 136,
      headerHeight: 30,
      rowCount: 100,
      rowHeight: 30,
      scrollbarSize: 16,
      viewportHeight: 400,
      viewportWidth: 700,
    });

    expect(layout.columnWidths).toEqual([280, 280, 280]);
    expect(layout.tableContentWidth).toBe(976);
    expect(layout.renderedTableWidth).toBe(992);
    expect(layout.bodyHeight).toBe(354);
    expect(layout.hasHorizontalScrollbar).toBe(true);
    expect(layout.hasVerticalScrollbar).toBe(true);
  });
});
