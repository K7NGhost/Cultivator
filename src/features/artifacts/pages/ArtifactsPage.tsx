import { ArtifactTreeViewer } from "../components/ArtifactTreeViewer";
import { PagedArtifactPanel } from "../components/PagedArtifactPanel";
import { useLocation } from "react-router-dom";
import { ArtifactDataPreview } from "../components/ArtifactDataPreview";
import type { ArtifactTreeNode } from "../model/artifactTree";
import { formatEntryHitLabel } from "../utils/artifactEntryLabel";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentProps } from "react";
import {
  AutoSizer,
  List,
  type ListRowProps,
} from "react-virtualized";
import "react-virtualized/styles.css";
import {
  AlertCircle,
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Check,
  ChevronDown,
  Database,
  RefreshCw,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useCases } from "@/features/cases";
import { listDataSources } from "@/features/datasources";
import type { DataSourceRecord } from "@/features/datasources";
import { artifactModelsByKind } from "../model/artifactModels";
import {
  ARTIFACT_NAVIGATION_CATEGORIES,
  getArtifactNavigationCategory,
} from "../model/artifactNavigationCategories";
import {
  deleteArtifact,
  deleteArtifacts,
  listArtifacts,
} from "../services/artifactRepository";
import {
  getArtifactEntryMetrics,
  getCustomTableRowSources,
  type CustomTableView,
  type CustomTableViewRow,
} from "../utils/customTableGrouping";
import { getCustomTableViewportLayout } from "../utils/customTableLayout";
import { getArtifactTableView } from "../utils/artifactTableView";
import { formatArtifactTableCell } from "../utils/artifactTableFormatting";
import { useTimeZone } from "@/features/settings/timeZoneSettings";
import { parseTimestamp } from "@/lib/timeZone";
import type {
  ArtifactModelDefinition,
  StoredArtifactRecord,
} from "../model/types";
import { listPythonPlugins } from "@/features/plugins";
import type { PythonPlugin } from "@/features/plugins";
import { cn } from "@/lib/utils";
import { useTaggedArtifactCatalog } from "../hooks/useTaggedArtifactCatalog";
import { filterTaggedArtifacts } from "../utils/taggedArtifactFilter";

type LoadState = {
  error: string | null;
  isLoading: boolean;
};

type ArtifactEntryViewMode = "selected" | "all";
type CustomTableSortDirection = "asc" | "desc";

type CustomTableSortState = {
  key: string;
  direction: CustomTableSortDirection;
};

const CUSTOM_TABLE_HEADER_HEIGHT = 30;
const CUSTOM_TABLE_ROW_HEIGHT = 30;
const CUSTOM_TABLE_MIN_COLUMN_WIDTH = 144;
const CUSTOM_TABLE_MAX_COLUMN_WIDTH = 280;
const CUSTOM_TABLE_SOURCES_COLUMN_WIDTH = 136;
const CUSTOM_TABLE_WORKER_SORT_THRESHOLD = 50_000;

type ArtifactCategoryBucket = {
  grouped: Map<string, { name: string; artifacts: StoredArtifactRecord[] }>;
  ungrouped: StoredArtifactRecord[];
};

type ArtifactRemovalTarget =
  | {
      kind: "artifact";
      name: string;
      artifacts: StoredArtifactRecord[];
    }
  | {
      kind: "category";
      category: string;
      artifacts: StoredArtifactRecord[];
    };

function useNativeScrollbarSize() {
  const [scrollbarSize, setScrollbarSize] = useState(0);

  useLayoutEffect(() => {
    if (typeof document === "undefined") {
      return;
    }

    const measurement = document.createElement("div");
    measurement.style.position = "fixed";
    measurement.style.width = "100px";
    measurement.style.height = "100px";
    measurement.style.overflow = "scroll";
    measurement.style.visibility = "hidden";
    measurement.style.pointerEvents = "none";
    document.body.appendChild(measurement);
    setScrollbarSize(measurement.offsetWidth - measurement.clientWidth);
    measurement.remove();
  }, []);

  return scrollbarSize;
}

function getErrorMessage(caughtError: unknown) {
  return caughtError instanceof Error ? caughtError.message : String(caughtError);
}

function formatDateTime(value: string, timeZone: string) {
  const recorded = parseTimestamp(value);
  if (recorded && recorded.basis !== "zoned") return value;
  const numericValue = Number(value);
  const date = Number.isFinite(numericValue)
    ? new Date(numericValue)
    : new Date(value);

  if (Number.isNaN(date.getTime())) {
    return value || "-";
  }

  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
    timeZone,
  }).format(date);
}

function getPayloadCategory(artifact: StoredArtifactRecord) {
  if (
    artifact.payload &&
    typeof artifact.payload === "object" &&
    !Array.isArray(artifact.payload) &&
    typeof artifact.payload.category === "string"
  ) {
    return artifact.payload.category;
  }

  return getArtifactModel(artifact.resultKind)?.category ?? "other";
}

function getPayloadIcon(artifact: StoredArtifactRecord) {
  if (!isPayloadObject(artifact.payload)) {
    return null;
  }

  const payload = artifact.payload as Record<string, unknown>;
  const payloadIcon = payload.icon;

  if (typeof payloadIcon === "string" && payloadIcon.length > 0) {
    return payloadIcon;
  }

  if (isPayloadObject(payload.table)) {
    const tableIcon = payload.table.icon;

    if (typeof tableIcon === "string" && tableIcon.length > 0) {
      return tableIcon;
    }
  }

  return null;
}

function getPayloadGroup(artifact: StoredArtifactRecord) {
  if (!isPayloadObject(artifact.payload) || !isPayloadObject(artifact.payload.group)) {
    return null;
  }

  const group = artifact.payload.group;
  const label =
    typeof group.label === "string" && group.label.length > 0
      ? group.label
      : typeof group.name === "string" && group.name.length > 0
        ? group.name
        : null;
  const id =
    typeof group.id === "string" && group.id.length > 0
      ? group.id
      : label;

  return id && label ? { id, label } : null;
}

function getArtifactModel(kind: string): ArtifactModelDefinition | undefined {
  return artifactModelsByKind.get(
    kind as ArtifactModelDefinition["kind"],
  );
}

function formatJson(value: unknown) {
  return JSON.stringify(value, null, 2) ?? "";
}

function isPayloadObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function getPayloadString(
  payload: Record<string, unknown>,
  key: string,
): string | null {
  const value = payload[key];

  return typeof value === "string" && value.length > 0 ? value : null;
}

function getPayloadStringList(
  payload: Record<string, unknown>,
  key: string,
): string[] {
  const value = payload[key];

  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter(
    (item): item is string => typeof item === "string" && item.length > 0,
  );
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

function isEmptyTableValue(value: unknown) {
  return value === null || value === undefined || value === "";
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
  sort: CustomTableSortState,
) {
  return Array.from({ length: rows.length }, (_, index) => index)
    .sort((leftIndex, rightIndex) => {
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
    });
}

function getNextCustomTableSort(
  current: CustomTableSortState,
  columnKey: string,
): CustomTableSortState {
  if (current.key !== columnKey) {
    return { key: columnKey, direction: "asc" };
  }

  return {
    key: columnKey,
    direction: current.direction === "asc" ? "desc" : "asc",
  };
}

function CustomTableSortIcon({
  columnKey,
  sort,
}: {
  columnKey: string;
  sort: CustomTableSortState;
}) {
  if (sort.key !== columnKey) {
    return (
      <ArrowUpDown className="size-3 text-muted-foreground" aria-hidden="true" />
    );
  }

  return sort.direction === "asc" ? (
    <ArrowUp className="size-3 text-foreground" aria-hidden="true" />
  ) : (
    <ArrowDown className="size-3 text-foreground" aria-hidden="true" />
  );
}

function useCustomTableSortedIndexes(
  rows: Record<string, unknown>[],
  sort: CustomTableSortState,
) {
  const workerRef = useRef<Worker | null>(null);
  const requestIdRef = useRef(0);
  const [sortedIndexes, setSortedIndexes] = useState<number[]>([]);
  const [isSorting, setIsSorting] = useState(false);

  useEffect(() => {
    return () => {
      workerRef.current?.terminate();
      workerRef.current = null;
    };
  }, []);

  useEffect(() => {
    requestIdRef.current += 1;
    const requestId = requestIdRef.current;

    if (!sort.key || rows.length === 0) {
      workerRef.current?.terminate();
      workerRef.current = null;
      setSortedIndexes([]);
      setIsSorting(false);
      return;
    }

    if (rows.length < CUSTOM_TABLE_WORKER_SORT_THRESHOLD) {
      workerRef.current?.terminate();
      workerRef.current = null;
      setIsSorting(false);
      setSortedIndexes(sortCustomTableRowIndexes(rows, sort));
      return;
    }

    workerRef.current?.terminate();
    const worker = new Worker(
      new URL("../workers/customTableSortWorker.ts", import.meta.url),
      { type: "module" },
    );
    workerRef.current = worker;
    setSortedIndexes([]);
    setIsSorting(true);

    worker.onmessage = (
      event: MessageEvent<{ id: number; sortedIndexes: number[] }>,
    ) => {
      if (event.data.id !== requestId) {
        return;
      }

      setSortedIndexes(event.data.sortedIndexes);
      setIsSorting(false);
    };

    worker.onerror = (event) => {
      if (requestIdRef.current !== requestId) {
        return;
      }

      console.error("Failed to sort custom table rows", event);
      setSortedIndexes([]);
      setIsSorting(false);
    };

    worker.postMessage({
      id: requestId,
      rows,
      sort,
    });

    return () => {
      if (workerRef.current === worker) {
        worker.terminate();
        workerRef.current = null;
      }
    };
  }, [rows, sort]);

  return { isSorting, sortedIndexes };
}

function formatArtifactFileLabel(artifact: StoredArtifactRecord) {
  if (!artifact.filePath) {
    return artifact.label || artifact.resultKind;
  }

  return formatFilePathLabel(artifact.filePath);
}

function formatFilePathLabel(filePath: string) {
  if (!filePath) {
    return "Unknown source";
  }

  const normalizedPath = filePath.replace(/\\/g, "/");
  const pathParts = normalizedPath.split("/").filter(Boolean);

  return pathParts[pathParts.length - 1] ?? filePath;
}

function getArtifactNodeKey(artifact: StoredArtifactRecord) {
  if (artifact.resultKind === "custom_table" && isPayloadObject(artifact.payload)) {
    const payload = artifact.payload as Record<string, unknown>;
    const table = payload.table;

    if (isPayloadObject(table) && typeof table.name === "string" && table.name.length > 0) {
      return [artifact.pluginId, artifact.resultKind, table.name].join(":");
    }
  }

  return [artifact.pluginId, artifact.resultKind].join(":");
}

function getArtifactNodeName(artifact: StoredArtifactRecord) {
  if (artifact.resultKind === "custom_table" && isPayloadObject(artifact.payload)) {
    const payload = artifact.payload as Record<string, unknown>;
    const table = payload.table;

    if (isPayloadObject(table) && typeof table.name === "string" && table.name.length > 0) {
      return table.name;
    }
  }

  return artifact.label || artifact.resultKind;
}

function buildArtifactNodes(
  category: string,
  artifacts: StoredArtifactRecord[],
  idPrefix: string,
  showEmptyArtifacts: boolean,
): ArtifactTreeNode[] {
  const artifactMap = new Map<string, StoredArtifactRecord[]>();

  for (const artifact of artifacts) {
    const key = getArtifactNodeKey(artifact);
    const artifactGroup = artifactMap.get(key) ?? [];

    artifactGroup.push(artifact);
    artifactMap.set(key, artifactGroup);
  }

  return Array.from(artifactMap.entries())
    .map(([groupKey, groupArtifacts]) => {
      const [firstArtifact] = groupArtifacts;
      const metrics = getArtifactEntryMetrics(groupArtifacts);

      return {
        id: `artifact:${category}:${idPrefix}:${groupKey}`,
        name: getArtifactNodeName(firstArtifact),
        kind: "artifact" as const,
        count: groupArtifacts.length,
        entryCount: metrics.entryCount,
        occurrenceCount: metrics.occurrenceCount,
        icon: getPayloadIcon(firstArtifact) ?? undefined,
        artifact: firstArtifact,
        artifacts: groupArtifacts,
      };
    })
    .filter((node) => showEmptyArtifacts || node.entryCount > 0)
    .sort((firstNode, secondNode) =>
      firstNode.name.localeCompare(secondNode.name),
    );
}

function buildArtifactTree(
  artifacts: StoredArtifactRecord[],
  showEmptyArtifacts: boolean,
): ArtifactTreeNode[] {
  const categoryMap = new Map<string, ArtifactCategoryBucket>(
    ARTIFACT_NAVIGATION_CATEGORIES.map((category) => [
      category,
      { grouped: new Map(), ungrouped: [] },
    ]),
  );

  for (const artifact of artifacts) {
    const category = getArtifactNavigationCategory(artifact);
    const categoryBucket = categoryMap.get(category);

    if (!categoryBucket) {
      continue;
    }

    const payloadGroup = getPayloadGroup(artifact);

    if (payloadGroup) {
      const groupKey = [artifact.pluginId, payloadGroup.id].join(":");
      const groupBucket =
        categoryBucket.grouped.get(groupKey) ?? {
          name: payloadGroup.label,
          artifacts: [],
        };

      groupBucket.artifacts.push(artifact);
      categoryBucket.grouped.set(groupKey, groupBucket);
    } else {
      categoryBucket.ungrouped.push(artifact);
    }
  }

  return Array.from(categoryMap.entries())
    .map(([category, categoryBucket]) => {
      const artifactNodes = buildArtifactNodes(
        category,
        categoryBucket.ungrouped,
        "ungrouped",
        showEmptyArtifacts,
      );
      const groupedArtifactNodes = Array.from(categoryBucket.grouped.entries())
        .flatMap(([groupKey, groupBucket]) =>
          buildArtifactNodes(
            category,
            groupBucket.artifacts,
            groupKey,
            showEmptyArtifacts,
          ),
        );
      const children = [...groupedArtifactNodes, ...artifactNodes].sort(
        (firstNode, secondNode) =>
          firstNode.name.localeCompare(secondNode.name),
      );

      return {
        id: `category:${category}`,
        name: category,
        kind: "category" as const,
        count: children.reduce((total, node) => total + node.count, 0),
        entryCount: children.reduce(
          (total, node) => total + node.entryCount,
          0,
        ),
        occurrenceCount: children.reduce(
          (total, node) => total + node.occurrenceCount,
          0,
        ),
        children,
      };
    });
}

function collectTreeArtifacts(node: ArtifactTreeNode): StoredArtifactRecord[] {
  if (node.artifacts) {
    return node.artifacts;
  }

  return node.children?.flatMap(collectTreeArtifacts) ?? [];
}

function findFirstArtifactNode(nodes: ArtifactTreeNode[]): ArtifactTreeNode | null {
  for (const node of nodes) {
    if (node.kind === "artifact") {
      return node;
    }

    const childArtifactNode = findFirstArtifactNode(node.children ?? []);

    if (childArtifactNode) {
      return childArtifactNode;
    }
  }

  return null;
}

function findArtifactNodeById(
  nodes: ArtifactTreeNode[],
  nodeId: string | null,
): ArtifactTreeNode | null {
  if (!nodeId) {
    return null;
  }

  for (const node of nodes) {
    if (node.id === nodeId && node.kind === "artifact") {
      return node;
    }

    const childArtifactNode = findArtifactNodeById(node.children ?? [], nodeId);

    if (childArtifactNode) {
      return childArtifactNode;
    }
  }

  return null;
}

function findArtifactNodeContainingArtifact(
  nodes: ArtifactTreeNode[],
  artifactId: string | null,
): ArtifactTreeNode | null {
  if (!artifactId) {
    return null;
  }

  for (const node of nodes) {
    if (
      node.kind === "artifact" &&
      node.artifacts?.some((artifact) => artifact.id === artifactId)
    ) {
      return node;
    }

    const childArtifactNode = findArtifactNodeContainingArtifact(
      node.children ?? [],
      artifactId,
    );

    if (childArtifactNode) {
      return childArtifactNode;
    }
  }

  return null;
}

export function ArtifactsPage() {
  const { activeCase } = useCases();
  const location = useLocation();
  const active = location.pathname === "/artifacts";
  const requestedArtifactId = new URLSearchParams(location.search).get("artifactId");
  const [artifacts, setArtifacts] = useState<StoredArtifactRecord[]>([]);
  const revision = artifacts[0]?.catalogRevision ?? "legacy";
  const loadVersion = useRef(0);
  const [plugins, setPlugins] = useState<PythonPlugin[]>([]);
  const [datasources, setDatasources] = useState<DataSourceRecord[]>([]);
  const [selectedDatasourceId, setSelectedDatasourceId] = useState("all");
  const [selectedArtifactId, setSelectedArtifactId] = useState<string | null>(
    null,
  );
  const [selectedArtifactNodeId, setSelectedArtifactNodeId] = useState<
    string | null
  >(null);
  const [showEmptyArtifacts, setShowEmptyArtifacts] = useState(false);
  const [taggedOnly, setTaggedOnly] = useState(false);
  const taggedCatalog = useTaggedArtifactCatalog(activeCase?.databasePath ?? "", revision, active);
  useEffect(() => { setTaggedOnly(false); }, [activeCase?.databasePath]);
  const [removalTarget, setRemovalTarget] =
    useState<ArtifactRemovalTarget | null>(null);
  const [loadState, setLoadState] = useState<LoadState>({
    error: null,
    isLoading: false,
  });
  const [deleteState, setDeleteState] = useState<LoadState>({
    error: null,
    isLoading: false,
  });
  const pluginMap = useMemo(() => {
    return new Map(plugins.map((plugin) => [plugin.id, plugin]));
  }, [plugins]);
  const datasourceMap = useMemo(() => {
    return new Map(datasources.map((datasource) => [datasource.id, datasource]));
  }, [datasources]);
  const visibleArtifacts = useMemo(
    () =>
      filterTaggedArtifacts(selectedDatasourceId === "all"
        ? artifacts
        : artifacts.filter(
            (artifact) => artifact.datasourceId === selectedDatasourceId,
          ), taggedOnly, taggedCatalog.counts),
    [artifacts, selectedDatasourceId, taggedOnly, taggedCatalog.counts],
  );
  const artifactTree = useMemo(
    () => buildArtifactTree(visibleArtifacts, showEmptyArtifacts).filter((node) => !taggedOnly || node.entryCount > 0),
    [visibleArtifacts, showEmptyArtifacts, taggedOnly],
  );
  const artifactEntryCount = artifactTree.reduce(
    (total, node) => total + node.entryCount,
    0,
  );
  const artifactOccurrenceCount = artifactTree.reduce(
    (total, node) => total + node.occurrenceCount,
    0,
  );
  const selectedArtifactNode =
    findArtifactNodeById(artifactTree, selectedArtifactNodeId) ??
    findArtifactNodeContainingArtifact(artifactTree, selectedArtifactId) ??
    (selectedArtifactId ? findFirstArtifactNode(artifactTree) : null);
  const selectedArtifactOptions = selectedArtifactNode?.artifacts ?? [];
  const selectedArtifact =
    selectedArtifactOptions.find((artifact) => artifact.id === selectedArtifactId) ??
    selectedArtifactOptions[0] ??
    null;

  useEffect(() => {
    if (requestedArtifactId && artifacts.some((artifact) => artifact.id === requestedArtifactId)) {
      setSelectedDatasourceId("all");
      setSelectedArtifactNodeId(null);
      setSelectedArtifactId(requestedArtifactId);
    }
  }, [requestedArtifactId, artifacts]);

  async function refreshArtifacts() {
    const version = ++loadVersion.current;
    if (!activeCase) {
      setArtifacts([]);
      setPlugins([]);
      setDatasources([]);
      setSelectedDatasourceId("all");
      setSelectedArtifactId(null);
      setSelectedArtifactNodeId(null);
      setLoadState({ error: null, isLoading: false });
      return;
    }

    setLoadState({ error: null, isLoading: true });
    setArtifacts([]);

    try {
      const [nextArtifacts, nextPlugins, nextDatasources] = await Promise.all([
        listArtifacts(activeCase.databasePath),
        listPythonPlugins(),
        listDataSources(activeCase.databasePath, activeCase.id),
      ]);
      if (version !== loadVersion.current) return;

      setArtifacts(nextArtifacts);
      setPlugins(nextPlugins);
      setDatasources(nextDatasources);
      setSelectedDatasourceId((currentId) =>
        currentId === "all" ||
        nextDatasources.some((datasource) => datasource.id === currentId)
          ? currentId
          : "all",
      );
      setSelectedArtifactId((currentId) => {
        if (nextArtifacts.some((artifact) => artifact.id === currentId)) {
          return currentId;
        }

        return null;
      });
      setSelectedArtifactNodeId(null);
      setLoadState({ error: null, isLoading: false });
    } catch (caughtError) {
      if (version !== loadVersion.current) return;
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    }
  }

  useEffect(() => {
    setArtifacts([]);
    if (active) void refreshArtifacts();
    return () => { loadVersion.current++; };
  }, [activeCase?.databasePath, active]);

  async function removeArtifacts() {
    if (!activeCase || !removalTarget) {
      return;
    }

    const removedArtifactIds =
      removalTarget.kind === "artifact"
        ? removalTarget.artifacts.map((artifact) => artifact.id)
        : removalTarget.artifacts.map((artifact) => artifact.id);
    const removedArtifactIdSet = new Set(removedArtifactIds);

    setDeleteState({ error: null, isLoading: true });

    try {
      if (removalTarget.kind === "artifact") {
        if (removedArtifactIds.length === 1) {
          await deleteArtifact(activeCase.databasePath, removedArtifactIds[0]);
        } else {
          await deleteArtifacts(activeCase.databasePath, removedArtifactIds);
        }
      } else {
        await deleteArtifacts(activeCase.databasePath, removedArtifactIds);
      }

      setArtifacts((currentArtifacts) => {
        const nextArtifacts = currentArtifacts.filter(
          (artifact) => !removedArtifactIdSet.has(artifact.id),
        );

        setSelectedArtifactId((currentId) => {
          if (!currentId || !removedArtifactIdSet.has(currentId)) {
            return currentId;
          }

          return null;
        });
        setSelectedArtifactNodeId(null);

        return nextArtifacts;
      });
      setRemovalTarget(null);
      setDeleteState({ error: null, isLoading: false });
    } catch (caughtError) {
      setDeleteState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <div className="text-xs font-medium uppercase text-muted-foreground">
          Artifacts
        </div>
        <Separator orientation="vertical" className="h-5" />
        <Button
          type="button"
          variant="outline"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          disabled={loadState.isLoading}
          onClick={() => {
            void refreshArtifacts();
          }}
        >
          <RefreshCw className="size-3.5" aria-hidden="true" />
          Refresh
        </Button>
        {loadState.isLoading && <span role="status" className="text-[11px] text-muted-foreground">Loading artifact catalog…</span>}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="h-7 max-w-56 justify-between gap-2 rounded-sm px-2 text-xs"
              disabled={!activeCase || datasources.length === 0}
            >
              <Database className="size-3.5 shrink-0" aria-hidden="true" />
              <span className="truncate">
                {selectedDatasourceId === "all"
                  ? "All datasources"
                  : datasourceMap.get(selectedDatasourceId)?.name ??
                    "All datasources"}
              </span>
              <ChevronDown className="size-3.5 shrink-0" aria-hidden="true" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="min-w-56">
            <DropdownMenuItem
              onSelect={() => {
                setSelectedDatasourceId("all");
                setSelectedArtifactNodeId(null);
                setSelectedArtifactId(null);
              }}
            >
              <Check
                className={cn(
                  "size-3.5",
                  selectedDatasourceId !== "all" && "opacity-0",
                )}
                aria-hidden="true"
              />
              All datasources
            </DropdownMenuItem>
            {datasources.map((datasource) => (
              <DropdownMenuItem
                key={datasource.id}
                onSelect={() => {
                  setSelectedDatasourceId(datasource.id);
                  setSelectedArtifactNodeId(null);
                  setSelectedArtifactId(null);
                }}
              >
                <Check
                  className={cn(
                    "size-3.5",
                    selectedDatasourceId !== datasource.id && "opacity-0",
                  )}
                  aria-hidden="true"
                />
                <span className="truncate">{datasource.name}</span>
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <div className="ml-auto flex items-center gap-2 text-[11px] text-muted-foreground">
          <span>
            {formatEntryHitLabel(
              artifactEntryCount,
              artifactOccurrenceCount,
              visibleArtifacts.length,
            )}
          </span>
          <Separator orientation="vertical" className="h-4" />
          <span>
            {(taggedOnly ? artifactTree.length : ARTIFACT_NAVIGATION_CATEGORIES.length).toLocaleString()} categories
          </span>
        </div>
      </section>

      {loadState.error && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{loadState.error}</span>
        </section>
      )}

      <ResizablePanelGroup
        orientation="horizontal"
        className="min-h-0 min-w-0 flex-1"
      >
        <ResizablePanel defaultSize="32%" minSize="24%">
          <ArtifactTreeViewer
            artifacts={visibleArtifacts}
            entryCount={artifactEntryCount}
            occurrenceCount={artifactOccurrenceCount}
            showEmptyArtifacts={showEmptyArtifacts}
            treeData={artifactTree}
            taggedOnly={taggedOnly}
            onTaggedOnlyChange={(checked) => {
              setTaggedOnly(checked); setSelectedArtifactNodeId(null);
              if (checked) setSelectedArtifactId(artifacts.find((item) => (selectedDatasourceId === "all" || item.datasourceId === selectedDatasourceId) && (taggedCatalog.counts.get(item.id) ?? 0) > 0)?.id ?? null);
            }}
            tagFilterLoading={taggedCatalog.loading}
            tagFilterError={taggedCatalog.error}
            onRetryTagFilter={taggedCatalog.refresh}
            selectedTreeNodeId={selectedArtifactNodeId}
            emptyText={
              activeCase
                ? taggedOnly ? !taggedCatalog.ready && taggedCatalog.loading ? "Loading tagged artifacts…" : taggedCatalog.error ? "Tagged artifacts could not be loaded." : "No tagged artifacts in the selected datasources."
                : visibleArtifacts.length > 0
                  ? "No artifacts with entries are visible."
                  : "No artifacts have been created by plugins yet."
                : "Create or select a case before viewing artifacts."
            }
            onShowEmptyArtifactsChange={setShowEmptyArtifacts}
            onSelectAllEvidence={() => {
              setSelectedArtifactNodeId(null);
              setSelectedArtifactId(null);
            }}
            onSelectArtifactNode={(artifactNode) => {
              const firstArtifact = artifactNode.artifacts?.[0] ?? artifactNode.artifact;

              setSelectedArtifactNodeId(artifactNode.id);
              setSelectedArtifactId(firstArtifact?.id ?? null);
            }}
            onRequestRemoveArtifact={(artifactNode) => {
              setDeleteState({ error: null, isLoading: false });
              setRemovalTarget({
                kind: "artifact",
                name: artifactNode.name,
                artifacts: collectTreeArtifacts(artifactNode),
              });
            }}
            onRequestRemoveCategory={(categoryNode) => {
              setDeleteState({ error: null, isLoading: false });
              setRemovalTarget({
                kind: "category",
                category: categoryNode.name,
                artifacts: collectTreeArtifacts(categoryNode),
              });
            }}
          />
        </ResizablePanel>

        <ResizableHandle withHandle />

        <ResizablePanel defaultSize="68%" minSize="34%">
          {active && <ArtifactPropertiesPanel
            databasePath={activeCase?.databasePath ?? ""}
            revision={revision}
            taggedOnly={taggedOnly}
            artifact={selectedArtifact}
            artifactOptions={selectedArtifactOptions}
            onSelectArtifact={(artifact) => setSelectedArtifactId(artifact.id)}
            datasourceName={
              selectedArtifact
                ? datasourceMap.get(selectedArtifact.datasourceId)?.name
                : undefined
            }
            pluginName={
              selectedArtifact
                ? pluginMap.get(selectedArtifact.pluginId)?.name
                : undefined
            }
          />}
        </ResizablePanel>
      </ResizablePanelGroup>

      <Dialog
        open={removalTarget !== null}
        onOpenChange={(open) => {
          if (!open && !deleteState.isLoading) {
            setRemovalTarget(null);
            setDeleteState({ error: null, isLoading: false });
          }
        }}
      >
        <DialogContent className="max-w-md gap-0 rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">
              {removalTarget?.kind === "category"
                ? "Remove category artifacts"
                : "Remove artifact"}
            </DialogTitle>
            <DialogDescription className="text-xs">
              {removalTarget?.kind === "category"
                ? "This removes every artifact in the selected category from the case database."
                : "This removes the selected artifact from the case database."}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2 px-3 py-3 text-xs">
            <div>
              {removalTarget?.kind === "category" ? (
                <>
                  Are you sure you want to remove all{" "}
                  <span className="font-medium">
                    {removalTarget.artifacts.length.toLocaleString()}
                  </span>{" "}
                  artifacts in{" "}
                  <span className="font-medium">{removalTarget.category}</span>?
                </>
              ) : (
                <>
                  Are you sure you want to remove{" "}
                  <span className="font-medium">
                    {removalTarget?.name || "this artifact"}
                  </span>
                  {removalTarget && removalTarget.artifacts.length > 1
                    ? ` across ${removalTarget.artifacts.length.toLocaleString()} files`
                    : ""}
                  ?
                </>
              )}
            </div>
            {deleteState.error && (
              <div className="rounded-sm border border-destructive/40 bg-destructive/10 px-2 py-1 text-destructive">
                {deleteState.error}
              </div>
            )}
          </div>
          <DialogFooter className="border-t px-3 py-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 rounded-sm px-2 text-xs"
              disabled={deleteState.isLoading}
              onClick={() => {
                setRemovalTarget(null);
                setDeleteState({ error: null, isLoading: false });
              }}
            >
              Cancel
            </Button>
            <Button
              type="button"
              variant="destructive"
              size="sm"
              className="h-8 rounded-sm px-2 text-xs"
              disabled={
                deleteState.isLoading ||
                (removalTarget?.kind === "category" &&
                  removalTarget.artifacts.length === 0)
              }
              onClick={() => {
                void removeArtifacts();
              }}
            >
              Remove
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function ArtifactPropertiesPanel({ databasePath, revision, taggedOnly, ...props }: ComponentProps<typeof LoadedArtifactPropertiesPanel> & {
  databasePath: string; revision: string; taggedOnly: boolean;
}) {
  if (props.artifact?.summaryOnly) return <PagedArtifactPanel
    key={`${props.artifactOptions[0]?.id}:${revision}`} databasePath={databasePath} revision={revision} taggedOnly={taggedOnly}
    pluginName={props.pluginName} datasourceName={props.datasourceName}
    artifact={props.artifact} artifacts={props.artifactOptions} onSelect={props.onSelectArtifact} />;
  return <LoadedArtifactPropertiesPanel {...props} />;
}

function LoadedArtifactPropertiesPanel({
  artifact,
  artifactOptions,
  datasourceName,
  onSelectArtifact,
  pluginName,
}: {
  artifact: StoredArtifactRecord | null;
  artifactOptions: StoredArtifactRecord[];
  datasourceName?: string;
  onSelectArtifact: (artifact: StoredArtifactRecord) => void;
  pluginName?: string;
}) {
  const timeZone = useTimeZone();
  const [entryViewMode, setEntryViewMode] =
    useState<ArtifactEntryViewMode>("all");
  const combinedTable = useMemo(() => getArtifactTableView(artifactOptions), [artifactOptions]);
  const selectedTable = useMemo(() => getArtifactTableView(artifact ? [artifact] : []), [artifact]);

  if (!artifact) {
    return (
      <section className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
        Select an artifact to inspect its properties.
      </section>
    );
  }

  const model = getArtifactModel(artifact.resultKind);
  const category = getPayloadCategory(artifact);
  const canViewAllEntries =
    artifactOptions.length > 1 && combinedTable !== null;
  const isViewingAllEntries = canViewAllEntries && entryViewMode === "all";
  const tableView = isViewingAllEntries ? combinedTable : selectedTable;
  const fileSelectorLabel = isViewingAllEntries
    ? "All entries"
    : formatArtifactFileLabel(artifact);
  const propertyRows = [
    ["Label", artifact.label || "-"],
    ["Kind", artifact.resultKind],
    ["Model", model?.label ?? "Custom"],
    ["Category", category],
    ["Plugin", pluginName ?? artifact.pluginId],
    ["Datasource", datasourceName ?? artifact.datasourceId],
    ["File", artifact.filePath || "-"],
    ["Created", formatDateTime(artifact.createdAt, timeZone)],
    ["Job", artifact.jobId],
  ];

  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
      <Tabs
        defaultValue="table"
        className="flex h-full min-h-0 min-w-0 flex-col gap-0"
      >
        <div className="flex h-8 shrink-0 items-center justify-between border-b px-2">
          <div className="flex min-w-0 items-center gap-2 text-xs font-medium uppercase text-muted-foreground">
            <Database className="size-3.5" aria-hidden="true" />
            Artifact
          </div>
          <div className="flex items-center gap-2">
            <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
              {category}
            </Badge>
            <TabsList
              variant="line"
              className="h-7 rounded-none p-0"
              aria-label="Artifact detail view"
            >
              <TabsTrigger
                value="table"
                className="h-7 rounded-none px-2 text-xs"
              >
                Table
              </TabsTrigger>
              <TabsTrigger
                value="preview"
                className="h-7 rounded-none px-2 text-xs"
              >
                Preview
              </TabsTrigger>
              <TabsTrigger
                value="properties"
                className="h-7 rounded-none px-2 text-xs"
              >
                Properties
              </TabsTrigger>
            </TabsList>
          </div>
        </div>

        {(artifactOptions.length > 1 || canViewAllEntries) && (
          <div className="flex h-10 shrink-0 items-center gap-2 border-b bg-muted/20 px-2">
            <div className="mr-1 shrink-0 text-[11px] font-medium uppercase text-muted-foreground">
              View
            </div>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 min-w-0 flex-1 justify-between rounded-sm px-2 text-xs"
                  title={
                    isViewingAllEntries
                      ? "All entries"
                      : artifact.filePath || artifact.label || artifact.resultKind
                  }
                >
                  <span className="min-w-0 truncate font-mono text-[11px]">
                    {fileSelectorLabel}
                  </span>
                  <ChevronDown className="size-3.5 shrink-0" aria-hidden="true" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="max-h-72 w-80">
                {canViewAllEntries && (
                  <DropdownMenuItem
                    className="grid grid-cols-[1rem_minmax(0,1fr)] gap-2 text-xs"
                    onSelect={() => setEntryViewMode("all")}
                  >
                    <span className="flex size-4 items-center justify-center">
                      {isViewingAllEntries ? (
                        <Check className="size-3.5" aria-hidden="true" />
                      ) : null}
                    </span>
                    <span className="min-w-0 truncate text-xs">
                      All entries
                    </span>
                  </DropdownMenuItem>
                )}
                {artifactOptions.map((option) => (
                  <DropdownMenuItem
                    key={option.id}
                    className="grid grid-cols-[1rem_minmax(0,1fr)] gap-2 text-xs"
                    title={option.filePath || option.label || option.resultKind}
                    onSelect={() => {
                      setEntryViewMode("selected");
                      onSelectArtifact(option);
                    }}
                  >
                    <span className="flex size-4 items-center justify-center">
                      {!isViewingAllEntries && option.id === artifact.id ? (
                        <Check className="size-3.5" aria-hidden="true" />
                      ) : null}
                    </span>
                    <span className="min-w-0 truncate font-mono text-[11px]">
                      {formatArtifactFileLabel(option)}
                    </span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <Badge variant="secondary" className="h-5 shrink-0 rounded-sm text-[11px]">
              {isViewingAllEntries
                ? combinedTable.rows.length.toLocaleString()
                : artifactOptions.length.toLocaleString()}
            </Badge>
          </div>
        )}

        <TabsContent
          value="table"
          className="m-0 min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          {tableView ? (
            <CustomTableArtifactPreview
              table={tableView}
              subtitle={isViewingAllEntries ? "All entries" : undefined}
              selectedArtifactId={artifact.resultKind !== "custom_table" ? artifact.id : undefined}
              onSelectArtifactId={artifact.resultKind !== "custom_table" ? (id) => {
                const selected = artifactOptions.find((option) => option.id === id);
                if (selected) onSelectArtifact(selected);
              } : undefined}
            />
          ) : (
            <div className="grid h-full place-items-center text-xs text-muted-foreground">
              No table data is available for this artifact.
            </div>
          )}
        </TabsContent>

        <TabsContent
          value="preview"
          className="m-0 min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <ArtifactPreviewPanel
            artifact={artifact}
            table={tableView}
            sourceArtifacts={artifactOptions}
            onSelectArtifactId={(id) => {
              const selected = artifactOptions.find((option) => option.id === id);
              if (selected) onSelectArtifact(selected);
            }}
          />
        </TabsContent>

        <TabsContent
          value="properties"
          className="m-0 min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <ScrollArea className="h-full min-h-0">
            <div className="divide-y text-xs">
              {propertyRows.map(([label, value]) => (
                <div
                  key={label}
                  className="grid grid-cols-[6.5rem_minmax(0,1fr)] gap-2 px-2 py-1.5"
                >
                  <div className="text-muted-foreground">{label}</div>
                  <div
                    className={cn(
                      "min-w-0 break-words",
                      label === "File" || label === "Job"
                        ? "font-mono text-[11px]"
                        : undefined,
                    )}
                  >
                    {value}
                  </div>
                </div>
              ))}
            </div>

            <div className="border-t">
              <div className="flex h-8 items-center border-b px-2 text-xs font-medium uppercase text-muted-foreground">
                Payload
              </div>
              <pre className="overflow-auto p-2 font-mono text-xs leading-5">
                {formatJson(artifact.payload)}
              </pre>
            </div>
          </ScrollArea>
        </TabsContent>
      </Tabs>
    </section>
  );
}

function ArtifactPreviewPanel({
  artifact,
  table,
  sourceArtifacts,
  onSelectArtifactId,
}: {
  artifact: StoredArtifactRecord;
  table: CustomTableView | null;
  sourceArtifacts: StoredArtifactRecord[];
  onSelectArtifactId: (id: string) => void;
}) {
  const timeZone = useTimeZone();
  if (table) {
    return <ArtifactDataPreview table={table} sourceArtifacts={sourceArtifacts} selectedArtifactId={artifact.id} onSelectArtifactId={onSelectArtifactId}
      fallback={<div className="p-3 text-xs text-muted-foreground">No specialized preview is available for these columns. Use Table to inspect the recorded values.</div>} />;
  }
  if (artifact.resultKind === "message" && isPayloadObject(artifact.payload)) {
    return <MessageArtifactPreview artifact={artifact} payload={artifact.payload} />;
  }

  return (
    <ScrollArea className="h-full min-h-0">
      <div className="space-y-2 p-3 text-xs">
        <div className="text-sm font-medium">{artifact.label || artifact.resultKind}</div>
        <div className="grid grid-cols-[6rem_1fr] gap-x-2 gap-y-1">
          <div className="text-muted-foreground">Kind</div>
          <div>{artifact.resultKind}</div>
          <div className="text-muted-foreground">File</div>
          <div className="min-w-0 break-words font-mono text-[11px]">
            {artifact.filePath || "-"}
          </div>
          <div className="text-muted-foreground">Created</div>
          <div>{formatDateTime(artifact.createdAt, timeZone)}</div>
        </div>
        <div className="rounded-sm border border-dashed px-2 py-2 text-[11px] text-muted-foreground">
          No specialized preview is available for this artifact kind.
        </div>
      </div>
    </ScrollArea>
  );
}

function CustomTableArtifactPreview({
  subtitle,
  table,
  selectedArtifactId,
  onSelectArtifactId,
}: {
  subtitle?: string;
  table: CustomTableView;
  selectedArtifactId?: string;
  onSelectArtifactId?: (id: string) => void;
}) {
  const timeZone = useTimeZone();
  const scrollbarSize = useNativeScrollbarSize();
  const [sort, setSort] = useState<CustomTableSortState>(() => ({
    key: table.columns[0]?.key ?? "",
    direction: "asc",
  }));
  useEffect(() => {
    if (table.columns.length === 0) {
      return;
    }

    setSort((current) =>
      table.columns.some((column) => column.key === current.key)
        ? current
        : { key: table.columns[0].key, direction: "asc" },
    );
  }, [table.columns]);
  const columnWidths = useMemo(
    () =>
      table.columns.map((column) =>
        Math.min(
          CUSTOM_TABLE_MAX_COLUMN_WIDTH,
          Math.max(
            CUSTOM_TABLE_MIN_COLUMN_WIDTH,
            column.label.length * 9 + 32,
          ),
        ),
      ),
    [table.columns],
  );
  const showSources =
    table.sources.length > 1 || table.totalOccurrences !== table.rows.length;
  const sortableRows = useMemo(
    () => table.rows.map((row) => row.values),
    [table.rows],
  );
  const { isSorting, sortedIndexes } = useCustomTableSortedIndexes(
    sortableRows,
    sort,
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-8 shrink-0 items-center justify-between border-b bg-muted/20 px-2 text-xs">
        <div className="min-w-0 truncate font-medium">
          {table.name}
          {subtitle ? (
            <span className="ml-2 font-normal text-muted-foreground">
              {subtitle}
            </span>
          ) : null}
        </div>
        <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
          {table.totalOccurrences === table.rows.length
            ? `${table.rows.length.toLocaleString()} rows`
            : `${table.rows.length.toLocaleString()} unique / ${table.totalOccurrences.toLocaleString()} occurrences`}
        </Badge>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden p-2">
        <div className="relative h-full overflow-hidden rounded-sm border">
          {table.rows.length === 0 ? (
            <div className="grid h-full place-items-center px-2 py-2 text-center text-xs text-muted-foreground">
              No rows were added to this table.
            </div>
          ) : (
            <AutoSizer>
              {({ height, width }) => {
                const viewportWidth = Math.max(0, width);
                const viewportHeight = Math.max(0, height);
                const layout = getCustomTableViewportLayout({
                  baseColumnWidths: columnWidths,
                  fixedTrailingWidth: showSources
                    ? CUSTOM_TABLE_SOURCES_COLUMN_WIDTH
                    : 0,
                  headerHeight: CUSTOM_TABLE_HEADER_HEIGHT,
                  rowCount: table.rows.length,
                  rowHeight: CUSTOM_TABLE_ROW_HEIGHT,
                  scrollbarSize,
                  viewportHeight,
                  viewportWidth,
                });
                const rowRenderer = ({
                  index,
                  key,
                  style,
                }: ListRowProps) => {
                  const sourceRowIndex = sortedIndexes[index] ?? index;
                  const groupedRow = table.rows[sourceRowIndex];
                  const sourceId = groupedRow ? table.sources[groupedRow.primarySourceIndex]?.artifactId : undefined;
                  const selected = selectedArtifactId !== undefined && sourceId === selectedArtifactId;
                  const selectRow = () => {
                    if (sourceId) onSelectArtifactId?.(sourceId);
                  };

                  return (
                    <div
                      key={key}
                      role="row"
                      aria-selected={onSelectArtifactId ? selected : undefined}
                      tabIndex={onSelectArtifactId ? 0 : undefined}
                      onClick={onSelectArtifactId ? selectRow : undefined}
                      onKeyDown={onSelectArtifactId ? (event) => {
                        if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
                          event.preventDefault();
                          selectRow();
                        }
                      } : undefined}
                      style={{ ...style, width: layout.tableContentWidth }}
                      className={cn(
                        "flex border-b text-xs hover:bg-muted/40",
                        index % 2 === 1 && "bg-muted/15",
                        onSelectArtifactId && "cursor-pointer focus-visible:outline focus-visible:outline-ring focus-visible:-outline-offset-2",
                        selected && "bg-primary/10",
                      )}
                    >
                      {table.columns.map((column, columnIndex) => {
                        const rawValue = formatTableCell(groupedRow?.values[column.key]);
                        const value = formatArtifactTableCell(
                          groupedRow?.values[column.key],
                          column,
                          timeZone,
                        );
                        const isLastDataColumn =
                          columnIndex === table.columns.length - 1;

                        return (
                          <div
                            key={column.key}
                            role="gridcell"
                            className={cn(
                              "flex h-full shrink-0 items-center px-2 text-[11px]",
                              (showSources || !isLastDataColumn) && "border-r",
                            )}
                            style={{ width: layout.columnWidths[columnIndex] }}
                            title={value === rawValue ? value : `${value}\nRaw: ${rawValue}`}
                          >
                            <span className="block min-w-0 truncate">
                              {value || "-"}
                            </span>
                          </div>
                        );
                      })}
                      {showSources && groupedRow ? (
                        <CustomTableSourcesCell
                          row={groupedRow}
                          table={table}
                        />
                      ) : null}
                    </div>
                  );
                };

                return (
                  <div
                    className="h-full overflow-x-auto overflow-y-hidden"
                    style={{ height: viewportHeight, width: viewportWidth }}
                  >
                    <div style={{ width: layout.renderedTableWidth }}>
                      <div
                        role="row"
                        className="flex bg-background text-xs"
                        style={{
                          height: CUSTOM_TABLE_HEADER_HEIGHT,
                          width: layout.renderedTableWidth,
                        }}
                      >
                        {table.columns.map((column, columnIndex) => {
                          const isLastDataColumn =
                            columnIndex === table.columns.length - 1;

                          return (
                            <div
                              key={column.key}
                              role="columnheader"
                              className={cn(
                                "flex h-full shrink-0 items-center border-b text-[11px] font-medium text-muted-foreground",
                                (showSources ||
                                  !isLastDataColumn ||
                                  layout.scrollbarGutterWidth > 0) &&
                                  "border-r",
                              )}
                              style={{
                                width: layout.columnWidths[columnIndex],
                              }}
                              title={column.label}
                            >
                              <Button
                                type="button"
                                variant="ghost"
                                size="xs"
                                className="h-full w-full justify-start gap-1 rounded-none px-2 text-[11px] font-medium uppercase text-muted-foreground"
                                aria-sort={
                                  sort.key === column.key
                                    ? sort.direction === "asc"
                                      ? "ascending"
                                      : "descending"
                                    : "none"
                                }
                                onClick={() => {
                                  setSort((current) =>
                                    getNextCustomTableSort(
                                      current,
                                      column.key,
                                    ),
                                  );
                                }}
                              >
                                <span className="min-w-0 truncate">
                                  {column.label}
                                </span>
                                <CustomTableSortIcon
                                  columnKey={column.key}
                                  sort={sort}
                                />
                              </Button>
                            </div>
                          );
                        })}
                        {showSources ? (
                          <div
                            role="columnheader"
                            className={cn(
                              "flex h-full shrink-0 items-center border-b px-2 text-[11px] font-medium uppercase text-muted-foreground",
                              layout.scrollbarGutterWidth > 0 && "border-r",
                            )}
                            style={{
                              width: CUSTOM_TABLE_SOURCES_COLUMN_WIDTH,
                            }}
                          >
                            Occurrences
                          </div>
                        ) : null}
                        {layout.scrollbarGutterWidth > 0 ? (
                          <div
                            aria-hidden="true"
                            className="h-full shrink-0 border-b bg-muted/20"
                            style={{ width: layout.scrollbarGutterWidth }}
                          />
                        ) : null}
                      </div>
                      <List
                        aria-label={`${table.name} artifact rows`}
                        height={layout.bodyHeight}
                        overscanRowCount={8}
                        rowCount={table.rows.length}
                        rowHeight={CUSTOM_TABLE_ROW_HEIGHT}
                        rowRenderer={rowRenderer}
                        style={{ overflowX: "hidden" }}
                        width={layout.renderedTableWidth}
                      />
                    </div>
                  </div>
                );
              }}
            </AutoSizer>
          )}
          {isSorting ? (
            <div className="absolute right-3 top-3 flex items-center gap-2 rounded-sm border bg-background/95 px-2 py-1 text-[11px] text-muted-foreground shadow-sm">
              <span
                className="size-4 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-foreground"
                aria-hidden="true"
              />
              Loading all entries
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function CustomTableSourcesCell({
  row,
  table,
}: {
  row: CustomTableViewRow;
  table: CustomTableView;
}) {
  const sources = getCustomTableRowSources(table, row);
  const occurrenceLabel = row.occurrenceCount === 1 ? "occurrence" : "occurrences";

  return (
    <div
      className="flex h-full shrink-0 items-center px-1.5"
      style={{ width: CUSTOM_TABLE_SOURCES_COLUMN_WIDTH }}
    >
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="h-6 w-full justify-between rounded-sm px-1.5 text-[11px] font-normal"
            title={`${row.occurrenceCount.toLocaleString()} ${occurrenceLabel} in ${sources.length.toLocaleString()} ${sources.length === 1 ? "source" : "sources"}`}
          >
            <span className="truncate">
              {row.occurrenceCount.toLocaleString()} {occurrenceLabel}
            </span>
            <ChevronDown className="size-3 shrink-0" aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="max-h-72 w-96">
          {sources.map((source) => (
            <DropdownMenuItem
              key={source.artifactId}
              className="grid grid-cols-[minmax(0,1fr)_auto] gap-2 text-xs"
              title={source.filePath || source.artifactId}
            >
              <span className="min-w-0 truncate font-mono text-[11px]">
                {formatFilePathLabel(source.filePath)}
              </span>
              <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
                {source.count.toLocaleString()}
              </Badge>
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function MessageArtifactPreview({
  artifact,
  payload,
}: {
  artifact: StoredArtifactRecord;
  payload: Record<string, unknown>;
}) {
  const timeZone = useTimeZone();
  const sender = getPayloadString(payload, "sender") ?? "Unknown sender";
  const recipients = getPayloadStringList(payload, "recipients");
  const body =
    getPayloadString(payload, "body") ??
    getPayloadString(payload, "message") ??
    artifact.label;
  const sentAt =
    getPayloadString(payload, "sentAt") ??
    getPayloadString(payload, "receivedAt") ??
    artifact.createdAt;
  const service = getPayloadString(payload, "service");
  const direction = getPayloadString(payload, "direction");
  const isOutgoing = direction === "outgoing";

  return (
    <div className="border-b bg-muted/20 px-3 py-3">
      <div className={cn("chat", isOutgoing ? "chat-end" : "chat-start")}>
        <div className="chat-header text-[11px] text-muted-foreground">
          <span className="font-medium text-foreground">{sender}</span>
          <time className="ml-2 opacity-70">{formatDateTime(sentAt, timeZone)}</time>
        </div>
        <div
          className={cn(
            "chat-bubble max-w-[min(34rem,100%)] whitespace-pre-wrap break-words text-xs leading-5",
            isOutgoing ? "chat-bubble-primary" : "chat-bubble-neutral",
          )}
        >
          {body || "(empty message)"}
        </div>
        <div className="chat-footer text-[11px] text-muted-foreground opacity-80">
          {recipients.length > 0 && <span>To: {recipients.join(", ")}</span>}
          {recipients.length > 0 && service && <span> | </span>}
          {service && <span>{service}</span>}
        </div>
      </div>
    </div>
  );
}
