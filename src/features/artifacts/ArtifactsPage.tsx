import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
  Car,
  Check,
  ChevronDown,
  ChevronRight,
  Clock,
  Contact,
  Database,
  FileText,
  FolderOpen,
  Image,
  MapPin,
  MessageSquare,
  RefreshCw,
  Search,
  Shield,
  Smartphone,
  Table2,
  Trash2,
  User,
  Wifi,
  type LucideIcon,
} from "lucide-react";
import {
  Tree,
  type NodeRendererProps,
  type TreeApi,
} from "react-arborist";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
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
import { useCases } from "@/features/cases/case-provider";
import { listDataSources } from "@/features/datasources/dataSourceRepository";
import type { DataSourceRecord } from "@/features/datasources/types";
import { artifactModelsByKind } from "@/features/artifacts/artifactModels";
import {
  ARTIFACT_NAVIGATION_CATEGORIES,
  getArtifactNavigationCategory,
} from "@/features/artifacts/artifactNavigationCategories";
import {
  deleteArtifact,
  deleteArtifacts,
  listArtifacts,
} from "@/features/artifacts/artifactRepository";
import {
  combineCustomTableArtifacts,
  getArtifactEntryMetrics,
  getCustomTableRowSources,
  type CustomTableView,
  type CustomTableViewRow,
} from "@/features/artifacts/customTableGrouping";
import { getCustomTableViewportLayout } from "@/features/artifacts/customTableLayout";
import type {
  ArtifactModelDefinition,
  StoredArtifactRecord,
} from "@/features/artifacts/types";
import { listPythonPlugins } from "@/features/plugins/pluginRepository";
import type { PythonPlugin } from "@/features/plugins/types";
import { cn } from "@/lib/utils";

type LoadState = {
  error: string | null;
  isLoading: boolean;
};

type ArtifactTreeNode = {
  id: string;
  name: string;
  kind: "category" | "group" | "artifact";
  count: number;
  entryCount: number;
  occurrenceCount: number;
  icon?: string;
  artifact?: StoredArtifactRecord;
  artifacts?: StoredArtifactRecord[];
  children?: ArtifactTreeNode[];
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

function useElementSize<TElement extends HTMLElement>() {
  const ref = useRef<TElement | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  useLayoutEffect(() => {
    const element = ref.current;

    if (!element) {
      return;
    }

    const resizeObserver = new ResizeObserver(([entry]) => {
      setSize({
        width: entry.contentRect.width,
        height: entry.contentRect.height,
      });
    });

    resizeObserver.observe(element);
    setSize({
      width: element.clientWidth,
      height: element.clientHeight,
    });

    return () => resizeObserver.disconnect();
  }, []);

  return { ref, size };
}

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

function formatDateTime(value: string) {
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

function getArtifactTreeIcon(iconName: string | undefined): LucideIcon {
  switch (iconName?.toLowerCase()) {
    case "car":
      return Car;
    case "clock":
      return Clock;
    case "contact":
    case "contacts":
      return Contact;
    case "image":
      return Image;
    case "map-pin":
    case "map":
      return MapPin;
    case "message":
    case "message-square":
      return MessageSquare;
    case "search":
      return Search;
    case "shield":
      return Shield;
    case "smartphone":
    case "phone":
      return Smartphone;
    case "table":
    case "table-2":
      return Table2;
    case "user":
      return User;
    case "wifi":
    case "wireless":
      return Wifi;
    default:
      return FileText;
  }
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

function getCombinedCustomTableData(
  artifacts: StoredArtifactRecord[],
): CustomTableView | null {
  return combineCustomTableArtifacts(artifacts);
}

function formatEntryHitLabel(
  entryCount: number,
  occurrenceCount: number,
  fileHits: number,
) {
  const entryLabel = entryCount === 1 ? "entry" : "entries";
  const occurrenceLabel = occurrenceCount === 1 ? "occurrence" : "occurrences";
  const fileLabel = fileHits === 1 ? "file hit" : "file hits";
  const entryText = `${entryCount.toLocaleString()} ${entryLabel}`;
  const fileText = `${fileHits.toLocaleString()} ${fileLabel}`;

  return occurrenceCount === entryCount
    ? `${entryText} / ${fileText}`
    : `${entryText} / ${occurrenceCount.toLocaleString()} ${occurrenceLabel} / ${fileText}`;
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
      new URL("./customTableSortWorker.ts", import.meta.url),
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
      const groupNodes = Array.from(categoryBucket.grouped.entries())
        .map(([groupKey, groupBucket]) => {
          const children = buildArtifactNodes(
            category,
            groupBucket.artifacts,
            groupKey,
            showEmptyArtifacts,
          );

          return {
            id: `group:${category}:${groupKey}`,
            name: groupBucket.name,
            kind: "group" as const,
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
        })
        .filter((node) => showEmptyArtifacts || node.entryCount > 0)
        .sort((firstNode, secondNode) =>
          firstNode.name.localeCompare(secondNode.name),
        );
      const children = [...groupNodes, ...artifactNodes];

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
  const [artifacts, setArtifacts] = useState<StoredArtifactRecord[]>([]);
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
      selectedDatasourceId === "all"
        ? artifacts
        : artifacts.filter(
            (artifact) => artifact.datasourceId === selectedDatasourceId,
          ),
    [artifacts, selectedDatasourceId],
  );
  const artifactTree = useMemo(
    () => buildArtifactTree(visibleArtifacts, showEmptyArtifacts),
    [visibleArtifacts, showEmptyArtifacts],
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
    findFirstArtifactNode(artifactTree);
  const selectedArtifactOptions = selectedArtifactNode?.artifacts ?? [];
  const selectedArtifact =
    selectedArtifactOptions.find((artifact) => artifact.id === selectedArtifactId) ??
    selectedArtifactOptions[0] ??
    null;

  async function refreshArtifacts() {
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

    try {
      const [nextArtifacts, nextPlugins, nextDatasources] = await Promise.all([
        listArtifacts(activeCase.databasePath),
        listPythonPlugins(),
        listDataSources(activeCase.databasePath, activeCase.id),
      ]);

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

        return nextArtifacts[0]?.id ?? null;
      });
      setSelectedArtifactNodeId(null);
      setLoadState({ error: null, isLoading: false });
    } catch (caughtError) {
      setLoadState({
        error: getErrorMessage(caughtError),
        isLoading: false,
      });
    }
  }

  useEffect(() => {
    void refreshArtifacts();
  }, [activeCase?.id]);

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

          return nextArtifacts[0]?.id ?? null;
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
            {ARTIFACT_NAVIGATION_CATEGORIES.length.toLocaleString()} categories
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
            selectedTreeNodeId={selectedArtifactNodeId}
            emptyText={
              activeCase
                ? visibleArtifacts.length > 0
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
          <ArtifactPropertiesPanel
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
          />
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

function ArtifactTreeViewer({
  artifacts,
  emptyText,
  entryCount,
  occurrenceCount,
  showEmptyArtifacts,
  onShowEmptyArtifactsChange,
  onSelectAllEvidence,
  onSelectArtifactNode,
  onRequestRemoveArtifact,
  onRequestRemoveCategory,
  selectedTreeNodeId,
  treeData,
}: {
  artifacts: StoredArtifactRecord[];
  emptyText: string;
  entryCount: number;
  occurrenceCount: number;
  showEmptyArtifacts: boolean;
  onShowEmptyArtifactsChange: (showEmptyArtifacts: boolean) => void;
  onSelectAllEvidence: () => void;
  onSelectArtifactNode: (artifactNode: ArtifactTreeNode) => void;
  onRequestRemoveArtifact: (artifactNode: ArtifactTreeNode) => void;
  onRequestRemoveCategory: (categoryNode: ArtifactTreeNode) => void;
  selectedTreeNodeId: string | null;
  treeData: ArtifactTreeNode[];
}) {
  const treePanel = useElementSize<HTMLDivElement>();
  const treeRef = useRef<TreeApi<ArtifactTreeNode> | undefined>(undefined);

  return (
    <section
      className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden border-r bg-background"
      aria-label="Artifact navigation"
    >
      <div className="flex h-10 shrink-0 items-end border-b px-2">
        <div className="flex h-full items-center border-b-2 border-primary px-3 text-[11px] font-semibold uppercase">
          Artifacts
        </div>
      </div>

      <ContextMenu>
        <ContextMenuTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            className={cn(
              "h-10 w-full shrink-0 justify-start rounded-none border-b px-6 text-sm font-semibold uppercase hover:bg-accent",
              selectedTreeNodeId === null &&
                "bg-primary/20 text-foreground hover:bg-primary/25 dark:bg-primary/25 dark:hover:bg-primary/30",
            )}
            title={formatEntryHitLabel(
              entryCount,
              occurrenceCount,
              artifacts.length,
            )}
            onClick={onSelectAllEvidence}
          >
            <span className="truncate">All Evidence</span>
            <span className="ml-auto tabular-nums">
              {entryCount.toLocaleString()}
            </span>
          </Button>
        </ContextMenuTrigger>
        <ContextMenuContent className="min-w-48">
          <ContextMenuCheckboxItem
            checked={showEmptyArtifacts}
            className="text-xs"
            onCheckedChange={(checked) => {
              onShowEmptyArtifactsChange(checked === true);
            }}
          >
            Show empty artifacts
          </ContextMenuCheckboxItem>
        </ContextMenuContent>
      </ContextMenu>

      <div ref={treePanel.ref} className="min-h-0 flex-1">
        {treeData.length === 0 ? (
          <div className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
            {emptyText}
          </div>
        ) : (
          treePanel.size.height > 0 && (
            <Tree
              ref={treeRef}
              data={treeData}
              width="100%"
              height={treePanel.size.height}
              rowHeight={40}
              indent={0}
              openByDefault={false}
              disableDrag
              disableDrop
              selection={selectedTreeNodeId ?? undefined}
              aria-label="Artifacts grouped by category"
            >
              {(props) => (
                <ArtifactTreeRow
                  {...props}
                  onSelectArtifactNode={onSelectArtifactNode}
                  onRequestRemoveArtifact={onRequestRemoveArtifact}
                  onRequestRemoveCategory={onRequestRemoveCategory}
                />
              )}
            </Tree>
          )
        )}
      </div>
    </section>
  );
}

function ArtifactTreeRow({
  node,
  onSelectArtifactNode,
  onRequestRemoveArtifact,
  onRequestRemoveCategory,
  style,
}: NodeRendererProps<ArtifactTreeNode> & {
  onSelectArtifactNode: (artifactNode: ArtifactTreeNode) => void;
  onRequestRemoveArtifact: (artifactNode: ArtifactTreeNode) => void;
  onRequestRemoveCategory: (categoryNode: ArtifactTreeNode) => void;
}) {
  const connectorWidth = node.level * 14;
  const isCategory = node.data.kind === "category";
  const displayName = isCategory
    ? node.data.name.toLowerCase() === "other"
      ? "Custom"
      : node.data.name.replace(/[_-]+/g, " ")
    : node.data.name;
  const Icon =
    node.data.kind === "artifact"
      ? getArtifactTreeIcon(node.data.icon)
      : node.data.kind === "group"
        ? FolderOpen
        : FileText;
  const iconClassName =
    node.data.kind === "artifact"
      ? "text-muted-foreground"
      : node.data.kind === "group"
        ? "text-amber-600 dark:text-amber-400"
        : "text-muted-foreground";
  const countTitle = formatEntryHitLabel(
    node.data.entryCount,
    node.data.occurrenceCount,
    node.data.count,
  );

  const row = (
    <div style={style}>
      <div
        className={cn(
          "flex h-full items-center border-b border-border/40",
          node.isSelected && !isCategory && "bg-primary/15",
        )}
        style={{ paddingLeft: `${isCategory ? 2 : connectorWidth + 2}px` }}
      >
        {node.isLeaf ? (
          <span className="size-6 shrink-0" aria-hidden="true" />
        ) : (
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="size-6 shrink-0 rounded-none"
            aria-label={
              node.isOpen ? "Collapse artifact group" : "Expand artifact group"
            }
            onClick={() => node.toggle()}
          >
            <ChevronRight
              className={cn(
                "size-3.5 text-muted-foreground transition-transform",
                node.isOpen && "rotate-90",
              )}
              aria-hidden="true"
            />
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
          className={cn(
            "h-full min-w-0 flex-1 justify-start gap-1.5 rounded-none px-1.5 hover:bg-accent",
            isCategory
              ? "text-sm font-semibold uppercase"
              : "text-xs font-medium",
          )}
          onClick={() => {
            if (node.data.kind === "artifact") {
              node.select();
              onSelectArtifactNode(node.data);
            } else if (node.isInternal) {
              node.toggle();
            }
          }}
        >
          {!isCategory ? (
            <Icon
              className={cn("size-3.5 shrink-0", iconClassName)}
              aria-hidden="true"
            />
          ) : null}
          <span className="min-w-0 flex-1 truncate text-left">
            {displayName}
          </span>
          <span
            className={cn(
              "shrink-0 tabular-nums",
              isCategory ? "text-sm font-semibold" : "text-xs font-medium",
            )}
            title={countTitle}
          >
            {node.data.entryCount.toLocaleString()}
          </span>
        </Button>
      </div>
    </div>
  );

  const artifact = node.data.artifact;

  if (node.data.kind === "category") {
    return (
      <ContextMenu>
        <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
        <ContextMenuContent className="min-w-48">
          <ContextMenuItem
            variant="destructive"
            disabled={node.data.count === 0}
            onSelect={() => {
              node.select();
              onRequestRemoveCategory(node.data);
            }}
          >
            <Trash2 className="size-3.5" aria-hidden="true" />
            Remove category artifacts
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    );
  }

  if (!artifact) {
    return row;
  }

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
      <ContextMenuContent className="min-w-44">
        <ContextMenuItem
          variant="destructive"
          onSelect={() => {
            node.select();
            onSelectArtifactNode(node.data);
            onRequestRemoveArtifact(node.data);
          }}
        >
          <Trash2 className="size-3.5" aria-hidden="true" />
          Remove artifact
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

function ArtifactPropertiesPanel({
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
  const [entryViewMode, setEntryViewMode] =
    useState<ArtifactEntryViewMode>("selected");

  if (!artifact) {
    return (
      <section className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
        Select an artifact to inspect its properties.
      </section>
    );
  }

  const model = getArtifactModel(artifact.resultKind);
  const category = getPayloadCategory(artifact);
  const combinedCustomTable = getCombinedCustomTableData(artifactOptions);
  const canViewAllEntries =
    artifactOptions.length > 1 && combinedCustomTable !== null;
  const isViewingAllEntries = canViewAllEntries && entryViewMode === "all";
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
    ["Created", formatDateTime(artifact.createdAt)],
    ["Job", artifact.jobId],
  ];

  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
      <Tabs
        defaultValue="preview"
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
                ? combinedCustomTable.rows.length.toLocaleString()
                : artifactOptions.length.toLocaleString()}
            </Badge>
          </div>
        )}

        <TabsContent
          value="preview"
          className="m-0 min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <ArtifactPreviewPanel
            artifact={artifact}
            artifactOptions={artifactOptions}
            entryViewMode={isViewingAllEntries ? "all" : "selected"}
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
  artifactOptions,
  entryViewMode,
}: {
  artifact: StoredArtifactRecord;
  artifactOptions: StoredArtifactRecord[];
  entryViewMode: ArtifactEntryViewMode;
}) {
  const combinedCustomTable =
    entryViewMode === "all" ? getCombinedCustomTableData(artifactOptions) : null;
  const customTable = combineCustomTableArtifacts([artifact]);

  if (combinedCustomTable) {
    return (
      <CustomTableArtifactPreview
        table={combinedCustomTable}
        subtitle="All entries"
      />
    );
  }

  if (customTable) {
    return <CustomTableArtifactPreview table={customTable} />;
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
          <div>{formatDateTime(artifact.createdAt)}</div>
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
}: {
  subtitle?: string;
  table: CustomTableView;
}) {
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

                  return (
                    <div
                      key={key}
                      role="row"
                      style={{ ...style, width: layout.tableContentWidth }}
                      className={cn(
                        "flex border-b text-xs hover:bg-muted/40",
                        index % 2 === 1 && "bg-muted/15",
                      )}
                    >
                      {table.columns.map((column, columnIndex) => {
                        const value = formatTableCell(
                          groupedRow?.values[column.key],
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
                            title={value}
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
          <time className="ml-2 opacity-70">{formatDateTime(sentAt)}</time>
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
