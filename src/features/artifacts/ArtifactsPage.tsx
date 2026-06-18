import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  ChevronRight,
  Database,
  FileText,
  FolderOpen,
  RefreshCw,
} from "lucide-react";
import {
  Tree,
  type NodeRendererProps,
  type TreeApi,
} from "react-arborist";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { listArtifacts } from "@/features/artifacts/artifactRepository";
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
  kind: "category" | "kind" | "artifact";
  count: number;
  artifact?: StoredArtifactRecord;
  children?: ArtifactTreeNode[];
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

function isPayloadObject(value: StoredArtifactRecord["payload"]): value is Record<string, unknown> {
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

function buildArtifactTree(artifacts: StoredArtifactRecord[]): ArtifactTreeNode[] {
  const categoryMap = new Map<string, Map<string, StoredArtifactRecord[]>>();

  for (const artifact of artifacts) {
    const category = getPayloadCategory(artifact);
    const kindMap = categoryMap.get(category) ?? new Map<string, StoredArtifactRecord[]>();
    const kindArtifacts = kindMap.get(artifact.resultKind) ?? [];

    kindArtifacts.push(artifact);
    kindMap.set(artifact.resultKind, kindArtifacts);
    categoryMap.set(category, kindMap);
  }

  return Array.from(categoryMap.entries())
    .sort(([firstCategory], [secondCategory]) =>
      firstCategory.localeCompare(secondCategory),
    )
    .map(([category, kindMap]) => {
      const kindNodes = Array.from(kindMap.entries())
        .sort(([firstKind], [secondKind]) => firstKind.localeCompare(secondKind))
        .map(([kind, kindArtifacts]) => ({
          id: `kind:${category}:${kind}`,
          name: getArtifactModel(kind)?.label ?? kind,
          kind: "kind" as const,
          count: kindArtifacts.length,
          children: kindArtifacts.map((artifact) => ({
            id: artifact.id,
            name: artifact.label || artifact.resultKind,
            kind: "artifact" as const,
            count: 0,
            artifact,
          })),
        }));

      return {
        id: `category:${category}`,
        name: category,
        kind: "category" as const,
        count: kindNodes.reduce((total, node) => total + node.count, 0),
        children: kindNodes,
      };
    });
}

export function ArtifactsPage() {
  const { activeCase } = useCases();
  const [artifacts, setArtifacts] = useState<StoredArtifactRecord[]>([]);
  const [plugins, setPlugins] = useState<PythonPlugin[]>([]);
  const [datasources, setDatasources] = useState<DataSourceRecord[]>([]);
  const [selectedArtifactId, setSelectedArtifactId] = useState<string | null>(
    null,
  );
  const [loadState, setLoadState] = useState<LoadState>({
    error: null,
    isLoading: false,
  });
  const pluginMap = useMemo(() => {
    return new Map(plugins.map((plugin) => [plugin.id, plugin]));
  }, [plugins]);
  const datasourceMap = useMemo(() => {
    return new Map(datasources.map((datasource) => [datasource.id, datasource]));
  }, [datasources]);
  const selectedArtifact =
    artifacts.find((artifact) => artifact.id === selectedArtifactId) ??
    artifacts[0] ??
    null;
  const categories = useMemo(() => {
    return artifacts.reduce((counts, artifact) => {
      const category = getPayloadCategory(artifact);

      counts.set(category, (counts.get(category) ?? 0) + 1);

      return counts;
    }, new Map<string, number>());
  }, [artifacts]);
  const artifactTree = useMemo(() => buildArtifactTree(artifacts), [artifacts]);

  async function refreshArtifacts() {
    if (!activeCase) {
      setArtifacts([]);
      setPlugins([]);
      setDatasources([]);
      setSelectedArtifactId(null);
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
            selectedArtifactId={selectedArtifact?.id ?? null}
            emptyText={
              activeCase
                ? "No artifacts have been created by plugins yet."
                : "Create or select a case before viewing artifacts."
            }
            onSelectArtifact={(artifact) => setSelectedArtifactId(artifact.id)}
          />
        </ResizablePanel>

        <ResizableHandle withHandle />

        <ResizablePanel defaultSize="62%" minSize="34%">
          <ArtifactPropertiesPanel
            artifact={selectedArtifact}
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
    </div>
  );
}

function ArtifactTreeViewer({
  artifacts,
  emptyText,
  onSelectArtifact,
  selectedArtifactId,
  treeData,
}: {
  artifacts: StoredArtifactRecord[];
  emptyText: string;
  onSelectArtifact: (artifact: StoredArtifactRecord) => void;
  selectedArtifactId: string | null;
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
              selection={selectedArtifactId ?? treeData[0]?.id}
              className="py-1"
              aria-label="Artifacts grouped by category and kind"
            >
              {(props) => (
                <ArtifactTreeRow
                  {...props}
                  onSelectArtifact={onSelectArtifact}
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
  onSelectArtifact,
  style,
}: NodeRendererProps<ArtifactTreeNode> & {
  onSelectArtifact: (artifact: StoredArtifactRecord) => void;
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

  return (
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

            if (node.data.artifact) {
              onSelectArtifact(node.data.artifact);
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
            <span className="max-w-32 truncate text-[10px] text-muted-foreground">
              {getPayloadSummary(node.data.artifact) ||
                formatDateTime(node.data.artifact.createdAt)}
            </span>
          ) : (
            <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
              {node.data.count}
            </Badge>
          )}
        </Button>
      </div>
    </div>
  );
}

function ArtifactPropertiesPanel({
  artifact,
  datasourceName,
  pluginName,
}: {
  artifact: StoredArtifactRecord | null;
  datasourceName?: string;
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
