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
  Tags,
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
import type { FileTagSummary } from "@/features/files/fileTagRepository";
import { cn } from "@/lib/utils";

type FileTreeViewerProps = {
  rootNode: EvidenceTreeNode | null;
  rootNodes?: EvidenceTreeNode[];
  selectedDirectory: EvidenceTreeNode | null;
  selectedFileView?: FileViewSelection | null;
  fileViewCounts?: Record<string, number>;
  tagSummaries?: FileTagSummary[];
  onSelectNode: (node: EvidenceTreeNode) => void;
  onSelectFileView?: (view: FileViewSelection) => void;
  onRemoveDataSource?: (node: EvidenceTreeNode) => void;
  onRunDataSourcePlugins?: (node: EvidenceTreeNode) => void;
};

export type FileViewSelection = {
  id: string;
  name: string;
  description: string;
  count: number;
  tagName?: string;
  childViews?: FileViewSelection[];
};

type AutopsyTreeNodeKind = EvidenceTreeNode["kind"] | "group" | "view";

type AutopsyTreeNode = Omit<EvidenceTreeNode, "children" | "kind"> & {
  children?: AutopsyTreeNode[];
  kind: AutopsyTreeNodeKind;
  sourceNode?: EvidenceTreeNode;
  fileView?: FileViewSelection;
};

type TreeOpenState = Record<string, boolean>;

const FILE_TREE_OPEN_STATE_STORAGE_KEY = "cultivator.fileTree.openState";

function loadFileTreeOpenState(): TreeOpenState {
  if (typeof localStorage === "undefined") {
    return {};
  }

  const storedValue = localStorage.getItem(FILE_TREE_OPEN_STATE_STORAGE_KEY);

  if (!storedValue) {
    return {};
  }

  try {
    const parsedValue = JSON.parse(storedValue);

    if (!parsedValue || typeof parsedValue !== "object" || Array.isArray(parsedValue)) {
      return {};
    }

    return Object.fromEntries(
      Object.entries(parsedValue).filter(
        (entry): entry is [string, boolean] =>
          typeof entry[0] === "string" && typeof entry[1] === "boolean",
      ),
    );
  } catch {
    return {};
  }
}

function saveFileTreeOpenState(openState: TreeOpenState) {
  if (typeof localStorage === "undefined") {
    return;
  }

  localStorage.setItem(
    FILE_TREE_OPEN_STATE_STORAGE_KEY,
    JSON.stringify(openState),
  );
}

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

function buildRealTreeNodes(
  nodes: EvidenceTreeNode[],
  options: { showFileLeaves: boolean },
): AutopsyTreeNode[] {
  const visibleNodes: AutopsyTreeNode[] = [];

  for (const node of nodes) {
    if (node.kind === "file" && !options.showFileLeaves) {
      continue;
    }

    const children = buildRealTreeNodes(node.children ?? [], options);

    visibleNodes.push({
      ...node,
      children: children.length > 0 ? children : undefined,
      sourceNode: node,
    });
  }

  return visibleNodes;
}

function createFileViewNode(
  id: string,
  name: string,
  description: string,
  counts: Record<string, number>,
  children?: AutopsyTreeNode[],
): AutopsyTreeNode {
  const childViews = children
    ?.map((child) => child.fileView)
    .filter((view): view is FileViewSelection => Boolean(view));
  const count = childViews?.length
    ? childViews.reduce((total, child) => total + child.count, 0)
    : counts[id] ?? 0;

  return {
    id: `autopsy:${id}`,
    name,
    path: "",
    kind: "view",
    files: count,
    children,
    fileView: {
      id,
      name,
      description,
      count,
      childViews,
    },
  };
}

function createTagViewNode(summary: FileTagSummary): AutopsyTreeNode {
  const encodedTagName = encodeURIComponent(summary.tagName);

  return {
    id: `autopsy:tag:${encodedTagName}`,
    name: summary.tagName,
    path: "",
    kind: "view",
    files: summary.count,
    fileView: {
      id: `tag:${encodedTagName}`,
      name: summary.tagName,
      description: `Files tagged ${summary.tagName}.`,
      count: summary.count,
      tagName: summary.tagName,
    },
  };
}

function buildAutopsyTree(
  sourceNodes: EvidenceTreeNode[],
  options: {
    fileViewCounts: Record<string, number>;
    showFileLeaves: boolean;
    tagSummaries: FileTagSummary[];
  },
): AutopsyTreeNode[] {
  const dataSourceChildren = buildRealTreeNodes(sourceNodes, options);
  const tagNodes = options.tagSummaries.map(createTagViewNode);
  const documentExtensionNodes = [
    createFileViewNode(
      "file-types:extension:documents:html",
      "HTML",
      "Files with .htm or .html extensions.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:documents:office",
      "Office",
      "Files with common Office document extensions.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:documents:pdf",
      "PDF",
      "Files with a .pdf extension.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:documents:text",
      "Plain Text",
      "Files with a .txt extension.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:documents:rtf",
      "RTF",
      "Files with a .rtf extension.",
      options.fileViewCounts,
    ),
  ];
  const executableExtensionNodes = [
    createFileViewNode(
      "file-types:extension:executables:exe",
      ".exe",
      "Windows executable files.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:executables:msi",
      ".msi",
      "Windows installer packages.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:executables:dll",
      ".dll",
      "Windows dynamic libraries.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:executables:bat",
      ".bat",
      "Windows batch scripts.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:executables:cmd",
      ".cmd",
      "Windows command scripts.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:executables:com",
      ".com",
      "DOS executable files.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:executables:reg",
      ".reg",
      "Windows registry scripts.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:executables:scr",
      ".scr",
      "Windows screen saver executables.",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:executables:ini",
      ".ini",
      "Windows initialization files.",
      options.fileViewCounts,
    ),
  ];
  const extensionNodes = [
    createFileViewNode(
      "file-types:extension:images",
      "Images",
      ".jpg, .jpeg, .png, .psd, .nef, .tiff, .bmp, .tcc, .tif, .webp",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:videos",
      "Videos",
      ".asf, .mov, .m1v, .m2v, .m4v, .mp4, .mpeg, .mpg, .mpe, .rm, .wmv, .mpv, .flv, .swf",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:audio",
      "Audio",
      ".aiff, .aif, .flac, .wav, .m4a, .ape, .wma, .mp2, .mp1, .mp3, .aac, .mp4, .m4p, .m1a, .m2a, .m4r, .mpa, .m3u, .mid, .midi, .ogg",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:archives",
      "Archives",
      ".zip, .rar, .7zip, .7z, .arj, .tar, .gzip, .bzip, .bzip2, .cab, .jar, .cpio, .ar, .gz, .tgz, .bz2",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:databases",
      "Databases",
      ".db, .db3, .sqlite, .sqlite3",
      options.fileViewCounts,
    ),
    createFileViewNode(
      "file-types:extension:documents",
      "Documents",
      ".htm, .html, .doc, .docx, .odt, .xls, .xlsx, .ppt, .pptx, .pdf, .txt, .rtf",
      options.fileViewCounts,
      documentExtensionNodes,
    ),
    createFileViewNode(
      "file-types:extension:executables",
      "Executable",
      ".exe, .msi, .cmd, .com, .bat, .reg, .scr, .dll, .ini",
      options.fileViewCounts,
      executableExtensionNodes,
    ),
  ];
  const mimeNodes = [
    createFileViewNode(
      "file-types:mime:image",
      "image",
      "Image MIME types.",
      options.fileViewCounts,
      [
        createFileViewNode(
          "file-types:mime:image:jpeg",
          "jpeg",
          "JPEG image files.",
          options.fileViewCounts,
        ),
        createFileViewNode(
          "file-types:mime:image:png",
          "png",
          "PNG image files.",
          options.fileViewCounts,
        ),
        createFileViewNode(
          "file-types:mime:image:gif",
          "gif",
          "GIF image files.",
          options.fileViewCounts,
        ),
        createFileViewNode(
          "file-types:mime:image:webp",
          "webp",
          "WebP image files.",
          options.fileViewCounts,
        ),
      ],
    ),
    createFileViewNode(
      "file-types:mime:video",
      "video",
      "Video MIME types.",
      options.fileViewCounts,
      [
        createFileViewNode(
          "file-types:mime:video:mp4",
          "mp4",
          "MP4 video files.",
          options.fileViewCounts,
        ),
      ],
    ),
    createFileViewNode(
      "file-types:mime:audio",
      "audio",
      "Audio MIME types.",
      options.fileViewCounts,
      [
        createFileViewNode(
          "file-types:mime:audio:mpeg",
          "mpeg",
          "MP3 audio files.",
          options.fileViewCounts,
        ),
      ],
    ),
    createFileViewNode(
      "file-types:mime:text",
      "text",
      "Text MIME types.",
      options.fileViewCounts,
      [
        createFileViewNode(
          "file-types:mime:text:plain",
          "plain",
          "Plain text files.",
          options.fileViewCounts,
        ),
        createFileViewNode(
          "file-types:mime:text:html",
          "html",
          "HTML text files.",
          options.fileViewCounts,
        ),
      ],
    ),
    createFileViewNode(
      "file-types:mime:application",
      "application",
      "Application MIME types.",
      options.fileViewCounts,
      [
        createFileViewNode(
          "file-types:mime:application:pdf",
          "pdf",
          "PDF application files.",
          options.fileViewCounts,
        ),
        createFileViewNode(
          "file-types:mime:application:zip",
          "zip",
          "ZIP archive files.",
          options.fileViewCounts,
        ),
      ],
    ),
  ];

  return [
    {
      id: "autopsy:data-sources",
      name: "Data Sources",
      path: "",
      kind: "group",
      files: dataSourceChildren.length,
      children: dataSourceChildren,
    },
    {
      id: "autopsy:file-views",
      name: "File Views",
      path: "",
      kind: "group",
      files: 3,
      children: [
        createFileViewNode(
          "file-types",
          "File Types",
          "Files grouped by extension and inferred MIME type.",
          options.fileViewCounts,
          [
            createFileViewNode(
              "file-types:extension",
              "By Extension",
              "Files grouped by Autopsy-style extension categories.",
              options.fileViewCounts,
              extensionNodes,
            ),
            createFileViewNode(
              "file-types:mime",
              "By MIME Type",
              "Files grouped by inferred MIME type.",
              options.fileViewCounts,
              mimeNodes,
            ),
          ],
        ),
        createFileViewNode(
          "deleted",
          "Deleted Files",
          "Files that Cultivator can infer as deleted from available filesystem data.",
          options.fileViewCounts,
          [
            createFileViewNode(
              "deleted:file-system",
              "File System",
              "Deleted files identified by filesystem metadata where available.",
              options.fileViewCounts,
            ),
            createFileViewNode(
              "deleted:all",
              "All",
              "All deleted-file markers Cultivator can infer from the source paths.",
              options.fileViewCounts,
            ),
          ],
        ),
        createFileViewNode(
          "file-size",
          "File Size",
          "Files grouped by Autopsy file-size ranges.",
          options.fileViewCounts,
          [
            createFileViewNode(
              "file-size:50-200mb",
              "50 - 200MB",
              "Files from 50,000,000 bytes up to 200,000,000 bytes.",
              options.fileViewCounts,
            ),
            createFileViewNode(
              "file-size:200mb-1gb",
              "200MB - 1GB",
              "Files from 200,000,000 bytes up to 1,000,000,000 bytes.",
              options.fileViewCounts,
            ),
            createFileViewNode(
              "file-size:1gb-plus",
              "1GB+",
              "Files at or above 1,000,000,000 bytes.",
              options.fileViewCounts,
            ),
          ],
        ),
      ],
    },
    {
      id: "autopsy:tags",
      name: "Tags",
      path: "",
      kind: "group",
      files: tagNodes.reduce((total, tagNode) => total + tagNode.files, 0),
      children: tagNodes.length > 0 ? tagNodes : undefined,
    },
  ];
}

function evidenceNodeToAutopsyNode(node: EvidenceTreeNode): AutopsyTreeNode {
  return {
    ...node,
    children: undefined,
    sourceNode: node,
  };
}

function findTreeNodeById(
  nodes: AutopsyTreeNode[],
  id: string,
): AutopsyTreeNode | null {
  for (const node of nodes) {
    if (node.id === id) {
      return node;
    }

    const childMatch = findTreeNodeById(node.children ?? [], id);

    if (childMatch) {
      return childMatch;
    }
  }

  return null;
}

function getNodeIcon(node: AutopsyTreeNode) {
  switch (node.kind) {
    case "group":
      return node.id === "autopsy:tags" ? Tags : FolderOpen;
    case "view":
      if (node.name === "Deleted Files") {
        return Trash2;
      }

      return node.children?.length ? FolderOpen : Folder;
    case "datasource":
      return Database;
    case "file":
      return File;
    case "directory":
      return node.children?.length ? FolderOpen : Folder;
  }
}

function getNodeIconClassName(node: AutopsyTreeNode) {
  switch (node.kind) {
    case "group":
      return node.id === "autopsy:tags"
        ? "text-sky-600 dark:text-sky-300"
        : "text-primary";
    case "view":
      return "text-muted-foreground";
    case "datasource":
      return "text-primary";
    case "file":
      return "text-muted-foreground";
    case "directory":
      return "text-amber-600 dark:text-amber-400";
  }
}

function getNodeKindLabel(node: AutopsyTreeNode) {
  switch (node.kind) {
    case "group":
      return "Category";
    case "view":
      return "View";
    case "datasource":
      return "Datasource";
    case "file":
      return "File";
    case "directory":
      return "Directory";
  }
}

function getNodeVisibleCount(node: AutopsyTreeNode) {
  return node.childCount ?? node.files;
}

function EvidenceTreeNodeRow({
  node,
  style,
  showChildCounts,
  onSelectNode,
  onRemoveDataSource,
  onRunDataSourcePlugins,
}: NodeRendererProps<AutopsyTreeNode> & {
  showChildCounts: boolean;
  onSelectNode: (node: AutopsyTreeNode) => void;
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
          {showChildCounts &&
            node.data.kind !== "file" &&
            (node.data.kind !== "view" || visibleCount > 0) && (
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
                  if (node.data.sourceNode) {
                    onRunDataSourcePlugins(node.data.sourceNode);
                  }
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
                  if (node.data.sourceNode) {
                    onRemoveDataSource(node.data.sourceNode);
                  }
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
  selectedFileView,
  fileViewCounts = {},
  tagSummaries = [],
  onSelectNode,
  onSelectFileView,
  onRemoveDataSource,
  onRunDataSourcePlugins,
}: FileTreeViewerProps) {
  const treePanel = useElementSize<HTMLDivElement>();
  const treeRef = useRef<TreeApi<AutopsyTreeNode> | undefined>(undefined);
  const selectedNodeRef = useRef<AutopsyTreeNode | null>(
    selectedDirectory ? evidenceNodeToAutopsyNode(selectedDirectory) : null,
  );
  const [selectedTreeNodeId, setSelectedTreeNodeId] = useState<string | null>(
    selectedDirectory?.id ?? null,
  );
  const [backStack, setBackStack] = useState<AutopsyTreeNode[]>([]);
  const [forwardStack, setForwardStack] = useState<AutopsyTreeNode[]>([]);
  const [showChildCounts, setShowChildCounts] = useState(true);
  const [showFileLeaves, setShowFileLeaves] = useState(false);
  const [treeOpenState, setTreeOpenState] =
    useState<TreeOpenState>(loadFileTreeOpenState);
  const sourceTreeData = useMemo(
    () => rootNodes ?? (rootNode ? [rootNode] : []),
    [rootNode, rootNodes],
  );
  const treeData = useMemo(
    () =>
      buildAutopsyTree(sourceTreeData, {
        fileViewCounts,
        showFileLeaves,
        tagSummaries,
      }),
    [fileViewCounts, showFileLeaves, sourceTreeData, tagSummaries],
  );

  useEffect(() => {
    if (selectedFileView) {
      const fileViewNodeId = selectedFileView.tagName
        ? `autopsy:tag:${encodeURIComponent(selectedFileView.tagName)}`
        : `autopsy:${selectedFileView.id}`;
      selectedNodeRef.current = findTreeNodeById(treeData, fileViewNodeId);
      setSelectedTreeNodeId(fileViewNodeId);
      return;
    }

    if (selectedDirectory) {
      selectedNodeRef.current = evidenceNodeToAutopsyNode(selectedDirectory);
      setSelectedTreeNodeId(selectedDirectory.id);
      return;
    }

    selectedNodeRef.current = null;
    setSelectedTreeNodeId(null);
  }, [selectedDirectory, selectedFileView, treeData]);

  useEffect(() => {
    setBackStack([]);
    setForwardStack([]);
  }, [sourceTreeData]);

  useEffect(() => {
    saveFileTreeOpenState(treeOpenState);
  }, [treeOpenState]);

  function handleTreeToggle(id: string) {
    // react-arborist only reports the toggled node id. The tree defaults to
    // open nodes, so an absent value means "open" until the user closes it.
    setTreeOpenState((currentOpenState) => ({
      ...currentOpenState,
      [id]: !(currentOpenState[id] ?? true),
    }));
  }

  function selectNode(
    node: AutopsyTreeNode,
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

    if (node.fileView) {
      onSelectFileView?.(node.fileView);
      return;
    }

    if (node.sourceNode) {
      onSelectNode(node.sourceNode);
    }
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
          Directory Tree
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
            initialOpenState={treeOpenState}
            onToggle={handleTreeToggle}
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
