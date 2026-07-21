import type {
  ArtifactDeduplicationPolicy,
  CustomTableColumn,
  StoredArtifactRecord,
} from "@/features/artifacts/types";

export type CustomTableSource = {
  artifactId: string;
  filePath: string;
};

export type CustomTableSourceOccurrence = {
  count: number;
  sourceIndex: number;
};

export type CustomTableViewRow = {
  occurrenceCount: number;
  primarySourceIndex: number;
  sourceOccurrences?: CustomTableSourceOccurrence[];
  values: Record<string, unknown>;
};

export type CustomTableView = {
  columns: CustomTableColumn[];
  deduplication: Required<Pick<ArtifactDeduplicationPolicy, "mode">> &
    Pick<ArtifactDeduplicationPolicy, "identityFields">;
  name: string;
  rows: CustomTableViewRow[];
  sources: CustomTableSource[];
  totalOccurrences: number;
};

export type ArtifactEntryMetrics = {
  entryCount: number;
  occurrenceCount: number;
};

type ParsedCustomTable = {
  artifact: StoredArtifactRecord;
  columns: CustomTableColumn[];
  name: string;
  policy: ReturnType<typeof getDeduplicationPolicy>;
  rows: Record<string, unknown>[];
};

const PROVENANCE_FIELDS = new Set([
  "deduplication",
  "group",
  "raw",
  "source",
]);

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getDeduplicationPolicy(payload: unknown) {
  if (!isObject(payload) || !isObject(payload.deduplication)) {
    return { mode: "group" as const, identityFields: undefined };
  }

  const policy = payload.deduplication;
  const mode = policy.mode === "preserve" ? "preserve" : "group";
  const identityFields = Array.isArray(policy.identityFields)
    ? Array.from(
        new Set(
          policy.identityFields.filter(
            (field): field is string =>
              typeof field === "string" && field.length > 0,
          ),
        ),
      )
    : undefined;

  return {
    mode,
    identityFields:
      identityFields && identityFields.length > 0 ? identityFields : undefined,
  };
}

function parseCustomTable(artifact: StoredArtifactRecord): ParsedCustomTable | null {
  const payload: unknown = artifact.payload;

  if (!isObject(payload) || !isObject(payload.table)) {
    return null;
  }

  const table = payload.table;

  if (!Array.isArray(table.columns) || !Array.isArray(table.rows)) {
    return null;
  }

  const columns = table.columns
    .filter(isObject)
    .map((column) => ({
      key: typeof column.key === "string" ? column.key : "",
      label: typeof column.label === "string" ? column.label : "",
    }))
    .filter((column) => column.key.length > 0 && column.label.length > 0);

  if (columns.length === 0) {
    return null;
  }

  return {
    artifact,
    columns,
    name:
      typeof table.name === "string" && table.name.length > 0
        ? table.name
        : typeof payload.label === "string" && payload.label.length > 0
          ? payload.label
          : "Custom Table",
    policy: getDeduplicationPolicy(payload),
    rows: table.rows.filter(isObject),
  };
}

function stableValueKey(value: unknown): string {
  if (value === null) {
    return "null";
  }

  if (value === undefined) {
    return "undefined";
  }

  if (typeof value === "string") {
    return `string:${JSON.stringify(value)}`;
  }

  if (typeof value === "number") {
    if (Number.isNaN(value)) {
      return "number:NaN";
    }

    return `number:${Object.is(value, -0) ? "-0" : String(value)}`;
  }

  if (typeof value === "boolean") {
    return `boolean:${value}`;
  }

  if (Array.isArray(value)) {
    return `array:[${value.map(stableValueKey).join(",")}]`;
  }

  if (isObject(value)) {
    return `object:{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableValueKey(value[key])}`)
      .join(",")}}`;
  }

  return `${typeof value}:${String(value)}`;
}

function getRowIdentity(
  row: Record<string, unknown>,
  identityFields: string[],
) {
  return stableValueKey(identityFields.map((field) => [field, row[field]]));
}

function addSourceOccurrence(
  row: CustomTableViewRow,
  sourceIndex: number,
) {
  if (!row.sourceOccurrences) {
    row.sourceOccurrences = [
      { count: 1, sourceIndex: row.primarySourceIndex },
    ];
  }

  const existingSource = row.sourceOccurrences.find(
    (source) => source.sourceIndex === sourceIndex,
  );

  if (existingSource) {
    existingSource.count += 1;
  } else {
    row.sourceOccurrences.push({ count: 1, sourceIndex });
  }
}

export function combineCustomTableArtifacts(
  artifacts: StoredArtifactRecord[],
): CustomTableView | null {
  const tables = artifacts
    .map(parseCustomTable)
    .filter((table): table is ParsedCustomTable => table !== null);

  if (tables.length === 0) {
    return null;
  }

  const columns = new Map<string, CustomTableColumn>();

  for (const table of tables) {
    for (const column of table.columns) {
      if (!columns.has(column.key)) {
        columns.set(column.key, column);
      }
    }
  }

  const mergedColumns = Array.from(columns.values());
  const firstConfiguredIdentityFields = tables[0].policy.identityFields;
  const hasConsistentIdentityFields =
    firstConfiguredIdentityFields !== undefined &&
    tables.every(
      (table) =>
        stableValueKey(table.policy.identityFields) ===
        stableValueKey(firstConfiguredIdentityFields),
    ) &&
    tables.every((table) =>
      table.rows.every((row) =>
        firstConfiguredIdentityFields.every((field) =>
          Object.prototype.hasOwnProperty.call(row, field),
        ),
      ),
    );
  const configuredIdentityFields = hasConsistentIdentityFields
    ? firstConfiguredIdentityFields
    : undefined;
  const policy = {
    mode: tables.some((table) => table.policy.mode === "preserve")
      ? ("preserve" as const)
      : ("group" as const),
    identityFields: configuredIdentityFields,
  };
  const identityFields =
    configuredIdentityFields ?? mergedColumns.map(({ key }) => key);
  const sources = tables.map(({ artifact }) => ({
    artifactId: artifact.id,
    filePath: artifact.filePath,
  }));
  const rows: CustomTableViewRow[] = [];
  const rowIndexesByIdentity = new Map<string, number>();
  let totalOccurrences = 0;

  tables.forEach((table, sourceIndex) => {
    for (const values of table.rows) {
      totalOccurrences += 1;

      if (policy.mode === "preserve") {
        rows.push({ occurrenceCount: 1, primarySourceIndex: sourceIndex, values });
        continue;
      }

      const identity = getRowIdentity(values, identityFields);
      const existingIndex = rowIndexesByIdentity.get(identity);

      if (existingIndex === undefined) {
        rowIndexesByIdentity.set(identity, rows.length);
        rows.push({ occurrenceCount: 1, primarySourceIndex: sourceIndex, values });
        continue;
      }

      const existingRow = rows[existingIndex];
      existingRow.occurrenceCount += 1;
      addSourceOccurrence(existingRow, sourceIndex);
    }
  });

  return {
    columns: mergedColumns,
    deduplication: policy,
    name: tables[0].name,
    rows,
    sources,
    totalOccurrences,
  };
}

export function getCustomTableRowSources(
  table: CustomTableView,
  row: CustomTableViewRow,
) {
  const occurrences = row.sourceOccurrences ?? [
    { count: 1, sourceIndex: row.primarySourceIndex },
  ];

  return occurrences.map(({ count, sourceIndex }) => ({
    ...table.sources[sourceIndex],
    count,
  }));
}

function getArtifactIdentity(artifact: StoredArtifactRecord) {
  const payload: unknown = artifact.payload;
  const policy = getDeduplicationPolicy(payload);

  if (policy.mode === "preserve") {
    return `artifact:${artifact.id}`;
  }

  if (!isObject(payload)) {
    return stableValueKey(payload);
  }

  if (policy.identityFields) {
    return getRowIdentity(payload, policy.identityFields);
  }

  return stableValueKey(
    Object.fromEntries(
      Object.entries(payload).filter(([key]) => !PROVENANCE_FIELDS.has(key)),
    ),
  );
}

export function getArtifactEntryMetrics(
  artifacts: StoredArtifactRecord[],
): ArtifactEntryMetrics {
  const customTable = combineCustomTableArtifacts(artifacts);

  if (customTable) {
    return {
      entryCount: customTable.rows.length,
      occurrenceCount: customTable.totalOccurrences,
    };
  }

  return {
    entryCount: new Set(artifacts.map(getArtifactIdentity)).size,
    occurrenceCount: artifacts.length,
  };
}
