import { useLayoutEffect, useRef, useState } from "react";
import {
  Tree,
  type NodeApi,
  type NodeRendererProps,
  type TreeApi,
} from "react-arborist";
import {
  ChevronsDownUp,
  ChevronsUpDown,
  ChevronRight,
  Database,
  File,
  FolderOpen,
  Play,
  Trash2,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
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

function EvidenceTreeNodeRow({
  node,
  style,
  onSelectNode,
  onRemoveDataSource,
  onRunDataSourcePlugins,
}: NodeRendererProps<EvidenceTreeNode> & {
  onSelectNode: (node: EvidenceTreeNode) => void;
  onRemoveDataSource?: (node: EvidenceTreeNode) => void;
  onRunDataSourcePlugins?: (node: EvidenceTreeNode) => void;
}) {
  const ancestorColumns: NodeApi<EvidenceTreeNode>[] = [];
  let parent = node.parent;

  while (parent && !parent.isRoot) {
    ancestorColumns.unshift(parent);
    parent = parent.parent;
  }

  const connectorWidth = node.level * 16;
  const currentLineX = Math.max(connectorWidth - 8, 0);
  const Icon =
    node.data.kind === "datasource"
      ? Database
      : node.data.kind === "file"
        ? File
        : FolderOpen;
  const iconClassName =
    node.data.kind === "datasource"
      ? "text-primary"
      : node.data.kind === "file"
        ? "text-muted-foreground"
        : "text-amber-600 dark:text-amber-400";

  const row = (
    <div style={style} className="relative px-1">
      <div
        className="pointer-events-none absolute inset-y-0 left-1"
        style={{ width: `${connectorWidth}px` }}
        aria-hidden="true"
      >
        {ancestorColumns.map((ancestorNode, index) => {
          if (!ancestorNode.nextSibling) {
            return null;
          }

          return (
            <span
              key={ancestorNode.id}
              className="absolute top-0 bottom-0 w-px bg-foreground"
              style={{ left: `${index * 16 + 8}px` }}
            />
          );
        })}
        {node.level > 0 && (
          <>
            <span
              className="absolute top-0 h-1/2 w-px bg-foreground"
              style={{ left: `${currentLineX}px` }}
            />
            {node.nextSibling && (
              <span
                className="absolute bottom-0 h-1/2 w-px bg-foreground"
                style={{ left: `${currentLineX}px` }}
              />
            )}
            <span
              className="absolute top-1/2 h-px w-2 bg-foreground"
              style={{ left: `${currentLineX}px` }}
            />
          </>
        )}
      </div>
      <div
        className={cn(
          "flex h-7 items-center rounded-sm",
          node.isSelected && "bg-accent",
        )}
        style={{ paddingLeft: `${connectorWidth + 6}px` }}
      >
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-5 shrink-0 rounded-sm"
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
          className="h-7 min-w-0 flex-1 justify-start gap-1 rounded-sm px-1.5 text-xs font-normal"
          onClick={() => {
            node.select();
            onSelectNode(node.data);
          }}
        >
          <Icon className={cn("size-3.5 shrink-0", iconClassName)} aria-hidden="true" />
          <span className="min-w-0 flex-1 truncate text-left">
            {node.data.name}
          </span>
          <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
            {node.data.files}
          </Badge>
        </Button>
      </div>
    </div>
  );

  if (
    node.data.kind !== "datasource" ||
    (!onRemoveDataSource && !onRunDataSourcePlugins)
  ) {
    return row;
  }

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
          Open datasource
        </ContextMenuItem>
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
        <ContextMenuSeparator />
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
  const treeData = rootNodes ?? (rootNode ? [rootNode] : []);

  return (
    <section className="h-full min-h-0" aria-label="Directory tree">
      <div className="flex h-8 items-center justify-between gap-2 border-b px-2">
        <div className="text-xs font-medium uppercase text-muted-foreground">
          Evidence Tree
        </div>
        <div className="flex items-center gap-1">
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
            openByDefault={false}
            disableDrag
            disableDrop
            selection={selectedDirectory?.id ?? treeData[0]?.id}
            className="py-1"
            aria-label="Evidence directory tree"
          >
            {(props) => (
              <EvidenceTreeNodeRow
                {...props}
                onSelectNode={onSelectNode}
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
