import { useLayoutEffect, useRef, useState } from "react";
import { Tree, type NodeApi, type NodeRendererProps } from "react-arborist";
import {
  ChevronRight,
  CheckCircle2,
  Clock3,
  File,
  FileCode2,
  FileImage,
  FolderOpen,
  Search,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import { cn } from "@/lib/utils";

type EvidenceTreeNode = {
  id: string;
  name: string;
  files: number;
  children?: EvidenceTreeNode[];
};

const evidenceTreeData: EvidenceTreeNode[] = [
  {
    id: "users",
    name: "Users",
    files: 1842,
    children: [
      {
        id: "users/inter",
        name: "Inter",
        files: 713,
        children: [
          { id: "users/inter/downloads", name: "Downloads", files: 228 },
          {
            id: "users/inter/browser-profiles",
            name: "Browser Profiles",
            files: 96,
          },
          {
            id: "users/inter/application-data",
            name: "Application Data",
            files: 389,
          },
        ],
      },
    ],
  },
  { id: "program-data", name: "ProgramData", files: 545 },
  { id: "temp", name: "Temp", files: 74 },
];

const fileRows = [
  {
    name: "Login Data",
    path: "Users/Inter/AppData/Local/Browser/Profile/Login Data",
    type: "SQLite",
    size: "2.4 MB",
    modified: "2026-06-14 22:18",
    plugin: "Credential Store Parser",
    status: "Queued",
    icon: FileCode2,
  },
  {
    name: "History",
    path: "Users/Inter/AppData/Local/Browser/Profile/History",
    type: "SQLite",
    size: "18.7 MB",
    modified: "2026-06-15 09:42",
    plugin: "Browser History Extractor",
    status: "Parsed",
    icon: FileCode2,
  },
  {
    name: "IMG_2044.jpg",
    path: "Users/Inter/Pictures/Camera Roll/IMG_2044.jpg",
    type: "JPEG",
    size: "4.8 MB",
    modified: "2026-06-10 16:03",
    plugin: "EXIF Metadata Reader",
    status: "Parsed",
    icon: FileImage,
  },
  {
    name: "notes.txt",
    path: "Users/Inter/Documents/notes.txt",
    type: "Text",
    size: "18 KB",
    modified: "2026-06-12 13:27",
    plugin: "Keyword Scanner",
    status: "New",
    icon: File,
  },
  {
    name: "Preferences",
    path: "Users/Inter/AppData/Roaming/App/Preferences",
    type: "JSON",
    size: "42 KB",
    modified: "2026-06-11 08:12",
    plugin: "Config Extractor",
    status: "Ready",
    icon: FileCode2,
  },
];

const pluginQueue = [
  { name: "Browser History Extractor", target: "History", state: "Complete" },
  { name: "Credential Store Parser", target: "Login Data", state: "Queued" },
  { name: "EXIF Metadata Reader", target: "IMG_2044.jpg", state: "Complete" },
  { name: "Keyword Scanner", target: "notes.txt", state: "Ready" },
];

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
}: NodeRendererProps<EvidenceTreeNode>) {
  const ancestorColumns: NodeApi<EvidenceTreeNode>[] = [];
  let parent = node.parent;

  while (parent && !parent.isRoot) {
    ancestorColumns.unshift(parent);
    parent = parent.parent;
  }

  const connectorWidth = node.level * 16;
  const currentLineX = Math.max(connectorWidth - 8, 0);

  return (
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
      <Button
        type="button"
        variant="ghost"
        className={cn(
          "h-7 w-full justify-start gap-1 rounded-sm px-1.5 text-xs font-normal",
          node.isSelected && "bg-accent",
        )}
        style={{ paddingLeft: `${connectorWidth + 6}px` }}
        onClick={() => node.isInternal && node.toggle()}
      >
        <ChevronRight
          className={cn(
            "size-3 shrink-0 text-muted-foreground transition-transform",
            node.isOpen && "rotate-90",
            node.isLeaf && "invisible",
          )}
          aria-hidden="true"
        />
        <FolderOpen
          className="size-3.5 shrink-0 text-muted-foreground"
          aria-hidden="true"
        />
        <span className="min-w-0 flex-1 truncate">{node.data.name}</span>
        <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
          {node.data.files}
        </Badge>
      </Button>
    </div>
  );
}

export function FilesPage() {
  const selectedFile = fileRows[0];
  const SelectedIcon = selectedFile.icon;
  const treePanel = useElementSize<HTMLDivElement>();

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <Button size="xs" className="h-7 px-2 text-xs">
          <FolderOpen className="size-3.5" aria-hidden="true" />
          Open Directory
        </Button>
        <Separator orientation="vertical" className="h-5" />
        <div className="relative w-72">
          <Search
            className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            className="h-7 pl-7 text-xs"
            placeholder="Search logical files..."
          />
        </div>
        <div className="ml-auto flex items-center gap-1 text-[11px] text-muted-foreground">
          <span>0 directories mounted</span>
          <Separator orientation="vertical" className="h-4" />
          <span>4 plugins available</span>
        </div>
      </section>

      <div className="grid min-h-0 flex-1 grid-cols-[220px_minmax(520px,1fr)_280px]">
        <section className="min-h-0 border-r" aria-label="Directory tree">
          <div className="flex h-8 items-center border-b px-2 text-xs font-medium uppercase text-muted-foreground">
            Evidence Tree
          </div>
          <div ref={treePanel.ref} className="h-[calc(100%-2rem)]">
            {treePanel.size.height > 0 && (
              <Tree
                data={evidenceTreeData}
                width="100%"
                height={treePanel.size.height}
                rowHeight={28}
                indent={0}
                openByDefault
                disableDrag
                disableDrop
                selection="users/inter/downloads"
                className="py-1"
                aria-label="Evidence directory tree"
              >
                {EvidenceTreeNodeRow}
              </Tree>
            )}
          </div>
        </section>

        <section className="min-h-0 border-r" aria-label="Logical file table">
          <div className="flex h-8 items-center justify-between border-b px-2">
            <h1 className="text-xs font-medium uppercase text-muted-foreground">
              Logical Files
            </h1>
            <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
              Directory-only acquisition
            </Badge>
          </div>
          <ScrollArea className="h-[calc(100%-2rem)]">
            <Table>
              <TableHeader>
                <TableRow className="h-7">
                  <TableHead className="h-7 px-2 text-[11px]">Name</TableHead>
                  <TableHead className="h-7 px-2 text-[11px]">Type</TableHead>
                  <TableHead className="h-7 px-2 text-[11px]">Size</TableHead>
                  <TableHead className="h-7 px-2 text-[11px]">
                    Modified
                  </TableHead>
                  <TableHead className="h-7 px-2 text-[11px]">Plugin</TableHead>
                  <TableHead className="h-7 px-2 text-[11px]">Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {fileRows.map((file, index) => {
                  const Icon = file.icon;

                  return (
                    <TableRow
                      key={file.path}
                      data-state={index === 0 ? "selected" : undefined}
                      className="h-8"
                    >
                      <TableCell className="max-w-[240px] px-2 py-1 text-xs">
                        <div className="flex min-w-0 items-center gap-1.5">
                          <Icon
                            className="size-3.5 shrink-0 text-muted-foreground"
                            aria-hidden="true"
                          />
                          <div className="min-w-0">
                            <div className="truncate font-medium">
                              {file.name}
                            </div>
                            <div className="truncate text-[11px] text-muted-foreground">
                              {file.path}
                            </div>
                          </div>
                        </div>
                      </TableCell>
                      <TableCell className="px-2 py-1 text-xs">
                        {file.type}
                      </TableCell>
                      <TableCell className="px-2 py-1 text-xs">
                        {file.size}
                      </TableCell>
                      <TableCell className="px-2 py-1 text-xs">
                        {file.modified}
                      </TableCell>
                      <TableCell className="max-w-[160px] px-2 py-1 text-xs">
                        <span className="block truncate">{file.plugin}</span>
                      </TableCell>
                      <TableCell className="px-2 py-1 text-xs">
                        <Badge
                          variant={
                            file.status === "Parsed" ? "secondary" : "outline"
                          }
                          className="h-5 rounded-sm px-1.5 text-[11px]"
                        >
                          {file.status}
                        </Badge>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </ScrollArea>
        </section>

        <aside className="min-h-0" aria-label="Inspector">
          <ScrollArea className="h-full">
            <div className="space-y-3 p-2">
              <section>
                <div className="flex items-center gap-2">
                  <div className="flex size-7 items-center justify-center rounded-sm border bg-muted">
                    <SelectedIcon className="size-3.5" aria-hidden="true" />
                  </div>
                  <div className="min-w-0">
                    <h2 className="truncate text-xs font-semibold">
                      {selectedFile.name}
                    </h2>
                    <p className="text-[11px] text-muted-foreground">
                      {selectedFile.type} · {selectedFile.size}
                    </p>
                  </div>
                </div>
              </section>

              <Separator />

              <section>
                <div className="mb-2 text-xs font-medium uppercase text-muted-foreground">
                  Selected File
                </div>
                <dl className="grid grid-cols-[78px_1fr] gap-x-2 gap-y-1 text-[11px]">
                  <dt className="text-muted-foreground">Path</dt>
                  <dd className="break-all">{selectedFile.path}</dd>
                  <dt className="text-muted-foreground">Modified</dt>
                  <dd>{selectedFile.modified}</dd>
                  <dt className="text-muted-foreground">Extractor</dt>
                  <dd>{selectedFile.plugin}</dd>
                </dl>
              </section>

              <Separator />

              <section>
                <div className="mb-2 text-xs font-medium uppercase text-muted-foreground">
                  Plugin Queue
                </div>
                <div className="space-y-1">
                  {pluginQueue.map((job) => (
                    <div
                      key={job.name}
                      className="flex min-w-0 items-center gap-2 rounded-sm border px-2 py-1.5"
                    >
                      {job.state === "Complete" ? (
                        <CheckCircle2
                          className="size-3.5 shrink-0 text-emerald-600"
                          aria-hidden="true"
                        />
                      ) : (
                        <Clock3
                          className="size-3.5 shrink-0 text-amber-600"
                          aria-hidden="true"
                        />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-xs font-medium">
                          {job.name}
                        </div>
                        <div className="truncate text-[11px] text-muted-foreground">
                          {job.target}
                        </div>
                      </div>
                      <Badge
                        variant="outline"
                        className="h-4 rounded-sm px-1 text-[10px]"
                      >
                        {job.state}
                      </Badge>
                    </div>
                  ))}
                </div>
              </section>
            </div>
          </ScrollArea>
        </aside>
      </div>

      <footer className="flex h-6 shrink-0 items-center gap-3 border-t px-2 text-[11px] text-muted-foreground">
        <span>Ready</span>
        <span>Evidence: directory</span>
        <span>Files indexed: 0</span>
        <span>Plugin jobs: 0 running</span>
      </footer>
    </div>
  );
}
