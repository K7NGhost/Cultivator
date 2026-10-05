export type SearchReportFile = {
  file: string;
  hits: number;
  path: string;
  kind: string;
};

export type SearchReportStatus = "idle" | "searching" | "completed" | "cancelled";

export type BuildSearchReportInput = {
  completedAt: Date | null;
  elapsedMs: number | null;
  files: SearchReportFile[];
  hitCount: number;
  query: string;
  rootPath: string | null;
  scannedFiles: number | null;
  status: SearchReportStatus;
  totalFiles: number | null;
};

type SearchReportFileRow = {
  hits: number;
  kind: string;
  location: string;
  name: string;
};

const REPORT_COLUMNS = [
  { key: "name", label: "Name", width: 46 },
  { key: "location", label: "Location", width: 76 },
  { key: "modified", label: "Modified", width: 22 },
  { key: "size", label: "Size", width: 10 },
  { key: "type", label: "Type", width: 34 },
  { key: "hits", label: "Hits", width: 5 },
] as const;

export function buildSearchReport({
  completedAt,
  elapsedMs,
  files,
  hitCount,
  query,
  rootPath,
  scannedFiles,
  status,
  totalFiles,
}: BuildSearchReportInput) {
  const fileRows = buildFileRows(files);
  const lines = [
    "Search Criteria",
    "",
    `File name:        ${""}`,
    `Containing text:  ${query}`,
    `Look in:          ${rootPath ?? ""}`,
    "",
    "",
    "Search Statistics",
    "",
    `Found:      ${formatCount(fileRows.length)} ${fileRows.length === 1 ? "item" : "items"}`,
    `Text:       ${formatCount(hitCount)} ${hitCount === 1 ? "hit" : "hits"}`,
    `Searched:   ${formatNullableCount(scannedFiles)} items`,
    `Checked:    ${formatNullableCount(totalFiles)} items`,
    `Status:     ${formatStatus(status, elapsedMs)}`,
    `Completed:  ${completedAt ? formatDateTime(completedAt) : ""}`,
    "",
    "",
    buildReportHeader(),
    "",
    ...fileRows.map(formatReportRow),
  ];

  return lines.join("\n");
}

function buildFileRows(files: SearchReportFile[]): SearchReportFileRow[] {
  return files
    .map((file) => ({
      hits: file.hits,
      kind: formatType(file.kind),
      location: getDirectoryPath(file.path),
      name: file.file,
    }))
    .sort((first, second) =>
      first.location.localeCompare(second.location) ||
      first.name.localeCompare(second.name),
    );
}

function buildReportHeader() {
  return REPORT_COLUMNS.map((column) =>
    column.label.padEnd(column.width, " "),
  ).join(" ");
}

function formatReportRow(row: SearchReportFileRow) {
  return [
    truncateCell(row.name, REPORT_COLUMNS[0].width),
    truncateCell(row.location, REPORT_COLUMNS[1].width),
    "".padEnd(REPORT_COLUMNS[2].width, " "),
    "".padEnd(REPORT_COLUMNS[3].width, " "),
    truncateCell(row.kind, REPORT_COLUMNS[4].width),
    row.hits.toLocaleString().padEnd(REPORT_COLUMNS[5].width, " "),
  ].join(" ");
}

function truncateCell(value: string, width: number) {
  if (value.length <= width) {
    return value.padEnd(width, " ");
  }

  if (width <= 3) {
    return value.slice(0, width);
  }

  return `${value.slice(0, width - 3)}...`;
}

function getDirectoryPath(path: string) {
  const lastSlashIndex = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));

  if (lastSlashIndex === -1) {
    return "";
  }

  return path.slice(0, lastSlashIndex + 1);
}

function formatType(kind: string) {
  if (!kind || kind === "File") {
    return "File";
  }

  return `${kind} File`;
}

function formatCount(value: number) {
  return value.toLocaleString();
}

function formatNullableCount(value: number | null) {
  return value === null ? "" : formatCount(value);
}

function formatStatus(status: SearchReportStatus, elapsedMs: number | null) {
  const label = {
    cancelled: "Cancelled",
    completed: "Completed",
    idle: "Not run",
    searching: "Searching",
  }[status];

  if (elapsedMs === null || status === "idle") {
    return label;
  }

  return `${label} (${formatDuration(elapsedMs)})`;
}

function formatDuration(milliseconds: number) {
  if (milliseconds < 1000) {
    return `${milliseconds} ms`;
  }

  const seconds = Math.round(milliseconds / 1000);

  if (seconds < 60) {
    return `${seconds} secs`;
  }

  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;

  return `${minutes} min ${remainingSeconds} secs`;
}

function formatDateTime(date: Date) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "medium",
  }).format(date);
}
