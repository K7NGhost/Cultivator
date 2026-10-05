export type CustomTableViewportLayoutInput = {
  baseColumnWidths: number[];
  fixedTrailingWidth: number;
  headerHeight: number;
  rowCount: number;
  rowHeight: number;
  scrollbarSize: number;
  viewportHeight: number;
  viewportWidth: number;
};

export type CustomTableViewportLayout = {
  bodyHeight: number;
  columnWidths: number[];
  hasHorizontalScrollbar: boolean;
  hasVerticalScrollbar: boolean;
  renderedTableWidth: number;
  scrollbarGutterWidth: number;
  tableContentWidth: number;
};

export function getCustomTableViewportLayout({
  baseColumnWidths,
  fixedTrailingWidth,
  headerHeight,
  rowCount,
  rowHeight,
  scrollbarSize,
  viewportHeight,
  viewportWidth,
}: CustomTableViewportLayoutInput): CustomTableViewportLayout {
  const safeViewportHeight = Math.max(0, viewportHeight);
  const safeViewportWidth = Math.max(0, viewportWidth);
  const safeScrollbarSize = Math.max(0, scrollbarSize);
  const baseColumnsWidth = baseColumnWidths.reduce(
    (total, width) => total + Math.max(0, width),
    0,
  );
  const baseContentWidth = baseColumnsWidth + Math.max(0, fixedTrailingWidth);
  const rowsHeight = Math.max(0, rowCount) * Math.max(0, rowHeight);
  let hasHorizontalScrollbar = false;
  let hasVerticalScrollbar = false;

  for (let pass = 0; pass < 3; pass += 1) {
    const scrollbarGutterWidth = hasVerticalScrollbar ? safeScrollbarSize : 0;
    const nextHorizontalScrollbar =
      baseContentWidth + scrollbarGutterWidth > safeViewportWidth;
    const bodyHeight = Math.max(
      0,
      safeViewportHeight -
        Math.max(0, headerHeight) -
        (nextHorizontalScrollbar ? safeScrollbarSize : 0),
    );
    const nextVerticalScrollbar = rowsHeight > bodyHeight;

    if (
      nextHorizontalScrollbar === hasHorizontalScrollbar &&
      nextVerticalScrollbar === hasVerticalScrollbar
    ) {
      break;
    }

    hasHorizontalScrollbar = nextHorizontalScrollbar;
    hasVerticalScrollbar = nextVerticalScrollbar;
  }

  const scrollbarGutterWidth = hasVerticalScrollbar ? safeScrollbarSize : 0;
  hasHorizontalScrollbar =
    baseContentWidth + scrollbarGutterWidth > safeViewportWidth;
  const bodyHeight = Math.max(
    0,
    safeViewportHeight -
      Math.max(0, headerHeight) -
      (hasHorizontalScrollbar ? safeScrollbarSize : 0),
  );
  const tableContentWidth = Math.max(
    baseContentWidth,
    safeViewportWidth - scrollbarGutterWidth,
  );
  const distributableWidth = Math.max(
    0,
    tableContentWidth - Math.max(0, fixedTrailingWidth) - baseColumnsWidth,
  );
  const additionalColumnWidth =
    baseColumnWidths.length > 0
      ? distributableWidth / baseColumnWidths.length
      : 0;
  const columnWidths = baseColumnWidths.map(
    (width) => Math.max(0, width) + additionalColumnWidth,
  );

  return {
    bodyHeight,
    columnWidths,
    hasHorizontalScrollbar,
    hasVerticalScrollbar,
    renderedTableWidth: tableContentWidth + scrollbarGutterWidth,
    scrollbarGutterWidth,
    tableContentWidth,
  };
}
