import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  Tree,
  type NodeRendererProps,
  type TreeApi,
} from "react-arborist";
import {
  ArrowLeft,
  ArrowRight,
  ChevronsDownUp,
  ChevronsUpDown,
  ChevronRight,
  Database,
  File,
  Folder,
  FolderOpen,
  Play,
  Settings2,
  Trash2,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Separator } from "@/components/ui/separator";
import type { EvidenceTreeNode } from "@/features/evidence/evidence-provider";
import { cn } from "@/lib/utils";

type FileTreeViewerProps = {
  rootNode: EvidenceTreeNode | null;
  rootNodes?: EvidenceTreeNode[];
  selectedDirectory: EvidenceTreeNode | null;
  onSelectNode: (node: EvidenceTreeNode) => void;
  onRemoveDataSource?: (node: EvidenceTreeNode) => void;
  onRunDataSourcePlugins?: (node: EvidenceTreeNode) => void;
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

    return () => resizeObserver.disconnect();
  }, []);

  return { ref, size };
}

function filterTreeForAutopsyView(
  nodes: EvidenceTreeNode[],
  options: { showFileLeaves: boolean },
): EvidenceTreeNode[] {
  const visibleNodes: EvidenceTreeNode[] = [];

  for (const node of nodes) {
    if (node.kind === "file" && !options.showFileLeaves) {
      continue;
    }

    const children = filterTreeForAutopsyView(node.children ?? [], options);

    visibleNodes.push({
      ...node,
      children: children.length > 0 ? children : undefined,
    });
  }

  return visibleNodes;
}

function getNodeIcon(node: EvidenceTreeNode) {
  switch (node.kind) {
    case "datasource":
      return Database;
    case "file":
      return File;
    case "directory":
      return node.children?.length ? FolderOpen : Folder;
  }
}

function getNodeIconClassName(node: EvidenceTreeNode) {
  switch (node.kind) {
    case "datasource":
      return "text-primary";
    case "file":
      return "text-muted-foreground";
    case "directory":
      return "text-amber-600 dark:text-amber-400";
  }
}

function getNodeKindLabel(node: EvidenceTreeNode) {
  switch (node.kind) {
    case "datasource":
      return "Datasource";
    case "file":
      return "File";
    case "directory":
      return "Directory";
  }
}

function getNodeVisibleCount(node: EvidenceTreeNode) {
  return node.childCount ?? node.files;
}

function EvidenceTreeNodeRow({
  node,
  style,
  showChildCounts,
  onSelectNode,
  onRemoveDataSource,
  onRunDataSourcePlugins,
}: NodeRendererProps<EvidenceTreeNode> & {
  showChildCounts: boolean;
  onSelectNode: (node: EvidenceTreeNode) => void;
  onRemoveDataSource?: (node: EvidenceTreeNode) => void;
  onRunDataSourcePlugins?: (node: EvidenceTreeNode) => void;
}) {
  const Icon = getNodeIcon(node.data);
  const visibleCount = getNodeVisibleCount(node.data);

  const row = (
    <div style={style} className="px-1">
      <div
        className={cn(
          "flex h-6 items-center rounded-sm border border-transparent text-xs",
          node.isSelected && "border-border bg-accent",
        )}
        style={{ paddingLeft: `${node.level * 16 + 2}px` }}
      >
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-5 shrink-0 rounded-sm hover:bg-transparent"
          disabled={node.isLeaf || node.data.kind === "file"}
          aria-label={node.isOpen ? "Collapse item" : "Expand item"}
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
          className="h-6 min-w-0 flex-1 justify-start gap-1 rounded-sm px-1 text-xs font-normal hover:bg-transparent"
          onClick={() => {
            node.select();
            onSelectNode(node.data);
          }}
        >
          <Icon
            className={cn("size-3.5 shrink-0", getNodeIconClassName(node.data))}
            aria-hidden="true"
          />
          <span className="min-w-0 truncate text-left">
            {node.data.name}
          </span>
          {showChildCounts && node.data.kind !== "file" && (
            <span className="shrink-0 text-[11px] text-muted-foreground">
              ({visibleCount})
            </span>
          )}
        </Button>
      </div>
    </div>
  );

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
      <ContextMenuContent className="min-w-44">
        <ContextMenuItem
          className="text-xs"
          onSelect={() => {
            node.select();
            onSelectNode(node.data);
          }}
        >
          Open {getNodeKindLabel(node.data).toLowerCase()}
        </ContextMenuItem>
        <ContextMenuItem
          className="text-xs"
          onSelect={() => {
            node.close();
          }}
        >
          <ChevronsDownUp className="size-3.5" aria-hidden="true" />
          Collapse branch
        </ContextMenuItem>
        {node.data.kind === "datasource" && (
          <>
            <ContextMenuSeparator />
            {onRunDataSourcePlugins && (
              <ContextMenuItem
                className="text-xs"
                onSelect={() => {
                  onRunDataSourcePlugins(node.data);
                }}
              >
                <Play className="size-3.5" aria-hidden="true" />
                Run plugins...
              </ContextMenuItem>
            )}
            {onRemoveDataSource && (
              <ContextMenuItem
                className="text-xs text-destructive focus:text-destructive"
                onSelect={() => {
                  onRemoveDataSource(node.data);
                }}
              >
                <Trash2 className="size-3.5" aria-hidden="true" />
                Remove datasource
              </ContextMenuItem>
            )}
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function FileTreeViewer({
  rootNode,
  rootNodes,
  selectedDirectory,
  onSelectNode,
  onRemoveDataSource,
  onRunDataSourcePlugins,
}: FileTreeViewerProps) {
  const treePanel = useElementSize<HTMLDivElement>();
  const treeRef = useRef<TreeApi<EvidenceTreeNode> | undefined>(undefined);
  const selectedNodeRef = useRef<EvidenceTreeNode | null>(selectedDirectory);
  const [selectedTreeNodeId, setSelectedTreeNodeId] = useState<string | null>(
    selectedDirectory?.id ?? null,
  );
  const [backStack, setBackStack] = useState<EvidenceTreeNode[]>([]);
  const [forwardStack, setForwardStack] = useState<EvidenceTreeNode[]>([]);
  const [showChildCounts, setShowChildCounts] = useState(true);
  const [showFileLeaves, setShowFileLeaves] = useState(false);
  const sourceTreeData = useMemo(
    () => rootNodes ?? (rootNode ? [rootNode] : []),
    [rootNode, rootNodes],
  );
  const treeData = useMemo(
    () => filterTreeForAutopsyView(sourceTreeData, { showFileLeaves }),
    [showFileLeaves, sourceTreeData],
  );

  useEffect(() => {
    selectedNodeRef.current = selectedDirectory;
    setSelectedTreeNodeId(selectedDirectory?.id ?? null);
  }, [selectedDirectory]);

  useEffect(() => {
    setBackStack([]);
    setForwardStack([]);
  }, [sourceTreeData]);

  function selectNode(
    node: EvidenceTreeNode,
    options: { updateHistory: boolean } = { updateHistory: true },
  ) {
    const currentNode = selectedNodeRef.current;

    if (
      options.updateHistory &&
      currentNode &&
      currentNode.id !== node.id
    ) {
      setBackStack((currentBackStack) => [...currentBackStack, currentNode]);
      setForwardStack([]);
    }

    selectedNodeRef.current = node;
    setSelectedTreeNodeId(node.id);
    onSelectNode(node);
  }

  function goBack() {
    const previousNode = backStack[backStack.length - 1];

    if (!previousNode) {
      return;
    }

    const currentNode = selectedNodeRef.current;
    setBackStack((currentBackStack) => currentBackStack.slice(0, -1));

    if (currentNode && currentNode.id !== previousNode.id) {
      setForwardStack((currentForwardStack) => [
        ...currentForwardStack,
        currentNode,
      ]);
    }

    selectNode(previousNode, { updateHistory: false });
  }

  function goForward() {
    const nextNode = forwardStack[forwardStack.length - 1];

    if (!nextNode) {
      return;
    }

    const currentNode = selectedNodeRef.current;
    setForwardStack((currentForwardStack) => currentForwardStack.slice(0, -1));

    if (currentNode && currentNode.id !== nextNode.id) {
      setBackStack((currentBackStack) => [...currentBackStack, currentNode]);
    }

    selectNode(nextNode, { updateHistory: false });
  }

  return (
    <section className="h-full min-h-0" aria-label="Directory tree">
      <div className="flex h-8 items-center gap-1 border-b px-1.5">
        <div className="flex items-center gap-1">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-6 rounded-sm"
            disabled={backStack.length === 0}
            aria-label="Back"
            title="Back"
            onClick={goBack}
          >
            <ArrowLeft className="size-3.5" aria-hidden="true" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-6 rounded-sm"
            disabled={forwardStack.length === 0}
            aria-label="Forward"
            title="Forward"
            onClick={goForward}
          >
            <ArrowRight className="size-3.5" aria-hidden="true" />
          </Button>
        </div>
        <Separator orientation="vertical" className="mx-1 h-4" />
        <div className="min-w-0 flex-1 truncate px-1 text-xs font-medium uppercase text-muted-foreground">
          Data Sources
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-6 rounded-sm"
            disabled={treeData.length === 0}
            aria-label="Expand all folders"
            title="Expand all"
            onClick={() => {
              treeRef.current?.openAll();
            }}
          >
            <ChevronsUpDown className="size-3.5" aria-hidden="true" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-6 rounded-sm"
            disabled={treeData.length === 0}
            aria-label="Collapse all folders"
            title="Collapse all"
            onClick={() => {
              treeRef.current?.closeAll();
            }}
          >
            <ChevronsDownUp className="size-3.5" aria-hidden="true" />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="size-6 rounded-sm"
                aria-label="Tree view options"
                title="View options"
              >
                <Settings2 className="size-3.5" aria-hidden="true" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-44">
              <DropdownMenuCheckboxItem
                className="text-xs"
                checked={showChildCounts}
                onCheckedChange={(checked) => {
                  setShowChildCounts(checked === true);
                }}
              >
                Show child counts
              </DropdownMenuCheckboxItem>
              <DropdownMenuCheckboxItem
                className="text-xs"
                checked={showFileLeaves}
                onCheckedChange={(checked) => {
                  setShowFileLeaves(checked === true);
                }}
              >
                Show file leaves
              </DropdownMenuCheckboxItem>
              <DropdownMenuSeparator />
              <div className="px-2 py-1 text-[11px] text-muted-foreground">
                Files are listed in the center pane by default.
              </div>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <div ref={treePanel.ref} className="h-[calc(100%-2rem)]">
        {treePanel.size.height > 0 && (
          <Tree
            ref={treeRef}
            data={treeData}
            width="100%"
            height={treePanel.size.height}
            rowHeight={28}
            indent={0}
            openByDefault
            disableDrag
            disableDrop
            selection={selectedTreeNodeId ?? treeData[0]?.id}
            className="bg-background py-1"
            aria-label="Evidence directory tree"
          >
            {(props) => (
              <EvidenceTreeNodeRow
                {...props}
                showChildCounts={showChildCounts}
                onSelectNode={selectNode}
                onRemoveDataSource={onRemoveDataSource}
                onRunDataSourcePlugins={onRunDataSourcePlugins}
              />
            )}
          </Tree>
        )}
      </div>
    </section>
  );
}
