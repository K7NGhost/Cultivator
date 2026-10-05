type CustomTableSortDirection = "asc" | "desc";

type CustomTableSortRequest = {
  id: number;
  rows: Record<string, unknown>[];
  sort: {
    key: string;
    direction: CustomTableSortDirection;
  };
};

type CustomTableSortResponse = {
  id: number;
  sortedIndexes: number[];
};

function isEmptyTableValue(value: unknown) {
  return value === null || value === undefined || value === "";
}

function formatTableCell(value: unknown) {
  if (value === null || value === undefined) {
    return "";
  }

  if (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return String(value);
  }

  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function compareTableValues(left: unknown, right: unknown) {
  if (isEmptyTableValue(left) || isEmptyTableValue(right)) {
    if (isEmptyTableValue(left) && isEmptyTableValue(right)) {
      return 0;
    }

    return isEmptyTableValue(left) ? 1 : -1;
  }

  if (typeof left === "number" && typeof right === "number") {
    return left - right;
  }

  if (typeof left === "boolean" && typeof right === "boolean") {
    return Number(left) - Number(right);
  }

  return formatTableCell(left).localeCompare(formatTableCell(right), undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

function sortCustomTableRowIndexes(
  rows: Record<string, unknown>[],
  sort: CustomTableSortRequest["sort"],
) {
  return Array.from({ length: rows.length }, (_, index) => index).sort(
    (leftIndex, rightIndex) => {
      const leftValue = rows[leftIndex][sort.key];
      const rightValue = rows[rightIndex][sort.key];
      const emptyComparison = compareTableValues(leftValue, rightValue);

      if (
        emptyComparison !== 0 &&
        (isEmptyTableValue(leftValue) || isEmptyTableValue(rightValue))
      ) {
        return emptyComparison;
      }

      const comparison = compareTableValues(leftValue, rightValue);

      if (comparison === 0) {
        return leftIndex - rightIndex;
      }

      return sort.direction === "asc" ? comparison : -comparison;
    },
  );
}

self.onmessage = (event: MessageEvent<CustomTableSortRequest>) => {
  const { id, rows, sort } = event.data;
  const response: CustomTableSortResponse = {
    id,
    sortedIndexes: sort.key
      ? sortCustomTableRowIndexes(rows, sort)
      : Array.from({ length: rows.length }, (_, index) => index),
  };

  self.postMessage(response);
};

export {};
