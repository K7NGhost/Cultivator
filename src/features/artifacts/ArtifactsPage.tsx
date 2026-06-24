import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  ChevronRight,
  Database,
  FileText,
  FolderOpen,
  RefreshCw,
  Trash2,
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
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useCases } from "@/features/cases/case-provider";
import { listDataSources } from "@/features/datasources/dataSourceRepository";
import type { DataSourceRecord } from "@/features/datasources/types";
import { artifactModelsByKind } from "@/features/artifacts/artifactModels";
import {
  deleteArtifact,
  deleteArtifacts,
  listArtifacts,
} from "@/features/artifacts/artifactRepository";
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
  kind: "category" | "artifact";
  count: number;
  artifact?: StoredArtifactRecord;
  artifacts?: StoredArtifactRecord[];
  children?: ArtifactTreeNode[];
};

type CustomTableColumn = {
  key: string;
  label: string;
};

type CustomTableData = {
  name: string;
  columns: CustomTableColumn[];
  rows: Record<string, unknown>[];
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

function getArtifactModel(kind: string): ArtifactModelDefinition | undefined {
  return artifactModelsByKind.get(
    kind as ArtifactModelDefinition["kind"],
  );
}

function getPayloadSummary(artifact: StoredArtifactRecord) {
  if (!artifact.payload || typeof artifact.payload !== "object") {
    return "";
  }

  const payload = artifact.payload as Record<string, unknown>;
  const summaryKeys = [
    "name",
    "title",
    "url",
    "phone",
    "email",
    "sender",
    "service",
    "path",
    "key",
  ];

  return summaryKeys
    .map((key) => payload[key])
    .filter((value): value is string => typeof value === "string" && value.length > 0)
    .slice(0, 2)
    .join(" | ");
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

function getCustomTableData(payload: unknown): CustomTableData | null {
  if (!isPayloadObject(payload) || !isPayloadObject(payload.table)) {
    return null;
  }

  const table = payload.table;

  if (!Array.isArray(table.columns) || !Array.isArray(table.rows)) {
    return null;
  }

  const columns = table.columns
    .filter(isPayloadObject)
    .map((column) => ({
      key: typeof column.key === "string" ? column.key : "",
      label: typeof column.label === "string" ? column.label : "",
    }))
    .filter((column) => column.key.length > 0 && column.label.length > 0);

  if (columns.length === 0) {
    return null;
  }

  return {
    name:
      typeof table.name === "string" && table.name.length > 0
        ? table.name
        : getPayloadString(payload, "label") ?? "Custom Table",
    columns,
    rows: table.rows.filter(isPayloadObject),
  };
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

function formatArtifactFileLabel(artifact: StoredArtifactRecord) {
  if (!artifact.filePath) {
    return artifact.label || artifact.resultKind;
  }

  const normalizedPath = artifact.filePath.replace(/\\/g, "/");
  const pathParts = normalizedPath.split("/").filter(Boolean);

  return pathParts[pathParts.length - 1] ?? artifact.filePath;
}

function buildArtifactTree(artifacts: StoredArtifactRecord[]): ArtifactTreeNode[] {
  const categoryMap = new Map<string, Map<string, StoredArtifactRecord[]>>();

  for (const artifact of artifacts) {
    const category = getPayloadCategory(artifact);
    const groupMap = categoryMap.get(category) ?? new Map<string, StoredArtifactRecord[]>();
    const groupKey = [
      artifact.pluginId,
      artifact.resultKind,
    ].join(":");
    const groupArtifacts = groupMap.get(groupKey) ?? [];

    groupArtifacts.push(artifact);
    groupMap.set(groupKey, groupArtifacts);
    categoryMap.set(category, groupMap);
  }

  return Array.from(categoryMap.entries())
    .sort(([firstCategory], [secondCategory]) =>
      firstCategory.localeCompare(secondCategory),
    )
    .map(([category, groupMap]) => {
      const artifactNodes = Array.from(groupMap.entries())
        .map(([groupKey, groupArtifacts]) => {
          const [firstArtifact] = groupArtifacts;

          return {
            id: `artifact:${category}:${groupKey}`,
            name: firstArtifact.label || firstArtifact.resultKind,
            kind: "artifact" as const,
            count: groupArtifacts.length,
            artifact: firstArtifact,
            artifacts: groupArtifacts,
          };
        })
        .sort((firstNode, secondNode) =>
          firstNode.name.localeCompare(secondNode.name),
        );

      return {
        id: `category:${category}`,
        name: category,
        kind: "category" as const,
        count: artifactNodes.reduce((total, node) => total + node.count, 0),
        children: artifactNodes,
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
  const [selectedArtifactId, setSelectedArtifactId] = useState<string | null>(
    null,
  );
  const [selectedArtifactNodeId, setSelectedArtifactNodeId] = useState<
    string | null
  >(null);
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
  const categories = useMemo(() => {
    return artifacts.reduce((counts, artifact) => {
      const category = getPayloadCategory(artifact);

      counts.set(category, (counts.get(category) ?? 0) + 1);

      return counts;
    }, new Map<string, number>());
  }, [artifacts]);
  const artifactTree = useMemo(() => buildArtifactTree(artifacts), [artifacts]);
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
        <div className="ml-auto flex items-center gap-2 text-[11px] text-muted-foreground">
          <span>{artifacts.length.toLocaleString()} artifacts</span>
          <Separator orientation="vertical" className="h-4" />
          <span>{categories.size.toLocaleString()} categories</span>
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
        <ResizablePanel defaultSize="38%" minSize="24%">
          <ArtifactTreeViewer
            artifacts={artifacts}
            treeData={artifactTree}
            selectedTreeNodeId={selectedArtifactNode?.id ?? null}
            emptyText={
              activeCase
                ? "No artifacts have been created by plugins yet."
                : "Create or select a case before viewing artifacts."
            }
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

        <ResizablePanel defaultSize="62%" minSize="34%">
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
  onSelectArtifactNode,
  onRequestRemoveArtifact,
  onRequestRemoveCategory,
  selectedTreeNodeId,
  treeData,
}: {
  artifacts: StoredArtifactRecord[];
  emptyText: string;
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
      className="h-full min-h-0 min-w-0 overflow-hidden border-r"
      aria-label="Artifact tree"
    >
      <div className="flex h-8 items-center justify-between border-b px-2">
        <div className="text-xs font-medium uppercase text-muted-foreground">
          Artifact Tree
        </div>
        <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
          {artifacts.length.toLocaleString()}
        </Badge>
      </div>
      <div ref={treePanel.ref} className="h-[calc(100%-2rem)]">
        {artifacts.length === 0 ? (
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
              rowHeight={30}
              indent={0}
              openByDefault
              disableDrag
              disableDrop
              selection={selectedTreeNodeId ?? treeData[0]?.id}
              className="py-1"
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
  const connectorWidth = node.level * 16;
  const Icon =
    node.data.kind === "artifact"
      ? FileText
      : node.data.kind === "category"
        ? Database
        : FolderOpen;
  const iconClassName =
    node.data.kind === "artifact"
      ? "text-muted-foreground"
      : node.data.kind === "category"
        ? "text-primary"
        : "text-amber-600 dark:text-amber-400";

  const row = (
    <div style={style} className="px-1">
      <div
        className={cn(
          "flex h-7 items-center rounded-sm",
          node.isSelected && "bg-accent",
        )}
        style={{ paddingLeft: `${connectorWidth + 4}px` }}
      >
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-5 shrink-0 rounded-sm"
          disabled={node.isLeaf}
          aria-label={node.isOpen ? "Collapse artifact group" : "Expand artifact group"}
          onClick={() => {
            if (node.isInternal) {
              node.toggle();
            }
          }}
        >
          <ChevronRight
            className={cn(
              "size-3 text-muted-foreground transition-transform",
              node.isOpen && "rotate-90",
              node.isLeaf && "invisible",
            )}
            aria-hidden="true"
          />
        </Button>
        <Button
          type="button"
          variant="ghost"
          className="h-7 min-w-0 flex-1 justify-start gap-1.5 rounded-sm px-1.5 text-xs font-normal"
          onClick={() => {
            node.select();

            if (node.data.kind === "artifact") {
              onSelectArtifactNode(node.data);
            } else if (node.isInternal) {
              node.toggle();
            }
          }}
        >
          <Icon className={cn("size-3.5 shrink-0", iconClassName)} aria-hidden="true" />
          <span className="min-w-0 flex-1 truncate text-left">
            {node.data.name}
          </span>
          {node.data.kind === "artifact" && node.data.artifact ? (
            node.data.count > 1 ? (
              <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
                {node.data.count} files
              </Badge>
            ) : (
              <span className="max-w-32 truncate text-[10px] text-muted-foreground">
                {getPayloadSummary(node.data.artifact) ||
                  formatDateTime(node.data.artifact.createdAt)}
              </span>
            )
          ) : (
            <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
              {node.data.count}
            </Badge>
          )}
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
  if (!artifact) {
    return (
      <section className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
        Select an artifact to inspect its properties.
      </section>
    );
  }

  const model = getArtifactModel(artifact.resultKind);
  const category = getPayloadCategory(artifact);
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

        {artifactOptions.length > 1 && (
          <div className="flex h-10 shrink-0 items-center gap-1 border-b bg-muted/20 px-2">
            <div className="mr-1 shrink-0 text-[11px] font-medium uppercase text-muted-foreground">
              File
            </div>
            <ScrollArea className="min-w-0 flex-1">
              <div className="flex min-w-max items-center gap-1">
                {artifactOptions.map((option) => (
                  <Button
                    key={option.id}
                    type="button"
                    variant={option.id === artifact.id ? "secondary" : "ghost"}
                    size="sm"
                    className="h-7 max-w-56 rounded-sm px-2 text-xs"
                    title={option.filePath || option.label || option.resultKind}
                    onClick={() => onSelectArtifact(option)}
                  >
                    <span className="truncate font-mono text-[11px]">
                      {formatArtifactFileLabel(option)}
                    </span>
                  </Button>
                ))}
              </div>
            </ScrollArea>
          </div>
        )}

        <TabsContent
          value="preview"
          className="m-0 min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <ArtifactPreviewPanel artifact={artifact} />
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
}: {
  artifact: StoredArtifactRecord;
}) {
  const customTable = getCustomTableData(artifact.payload);

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

function CustomTableArtifactPreview({ table }: { table: CustomTableData }) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-8 shrink-0 items-center justify-between border-b bg-muted/20 px-2 text-xs">
        <div className="min-w-0 truncate font-medium">{table.name}</div>
        <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
          {table.rows.length.toLocaleString()} rows
        </Badge>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden p-2">
        <Table
          containerClassName="h-full overflow-auto rounded-sm border"
          className="w-max min-w-full table-auto text-xs"
        >
          <TableHeader className="sticky top-0 z-10 bg-background">
            <TableRow className="hover:bg-transparent">
              {table.columns.map((column) => (
                <TableHead
                  key={column.key}
                  className="border-r px-2 py-1 align-top text-[11px] last:border-r-0"
                  title={column.label}
                >
                  <span className="block whitespace-nowrap">
                    {column.label}
                  </span>
                </TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {table.rows.map((row, rowIndex) => (
              <TableRow key={rowIndex}>
                {table.columns.map((column) => {
                  const value = formatTableCell(row[column.key]);

                  return (
                    <TableCell
                      key={column.key}
                      className="border-r px-2 py-1 align-top text-[11px] last:border-r-0"
                      title={value}
                    >
                      <span className="block whitespace-pre-wrap">
                        {value || "-"}
                      </span>
                    </TableCell>
                  );
                })}
              </TableRow>
            ))}

            {table.rows.length === 0 && (
              <TableRow>
                <TableCell
                  colSpan={table.columns.length}
                  className="h-12 px-2 py-2 text-center text-xs text-muted-foreground"
                >
                  No rows were added to this table.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
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
