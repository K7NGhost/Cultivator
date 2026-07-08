import { Database, FileCode2, Hexagon } from "lucide-react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";
import {
  ApplicationPreviewViewer,
  type FileFormatPreview,
} from "@/features/files/components/ApplicationPreviewViewer";
import { HexPreviewViewer } from "@/features/files/components/HexPreviewViewer";
import { TextPreviewViewer } from "@/features/files/components/TextPreviewViewer";

type FilePreviewViewerProps = {
  activeTab: FilePreviewTab;
  filePreview: FileFormatPreview | null;
  isLoading: boolean;
  onActiveTabChange: (tab: FilePreviewTab) => void;
  selectedEntry: EvidenceDirectoryEntry | null;
};

export type FilePreviewTab = "file" | "text" | "hex";
export type { FileFormatPreview };

export function FilePreviewViewer({
  activeTab,
  filePreview,
  isLoading,
  onActiveTabChange,
  selectedEntry,
}: FilePreviewViewerProps) {
  return (
    <section
      className="h-full min-h-0 min-w-0 overflow-hidden"
      aria-label="File preview"
    >
      <Tabs
        value={activeTab}
        onValueChange={(value) => {
          onActiveTabChange(value as FilePreviewTab);
        }}
        className="flex h-full min-h-0 min-w-0 flex-col gap-0"
      >
        <div className="flex h-8 items-center justify-between border-b px-2">
          <div className="flex min-w-0 items-center gap-2 text-xs">
            <span className="font-medium">Preview:</span>
            <span className="truncate text-muted-foreground">
              {selectedEntry?.path ?? "No file selected"}
            </span>
          </div>
          <TabsList
            variant="line"
            className="h-7 rounded-none p-0"
            aria-label="Preview mode"
          >
            <TabsTrigger
              value="text"
              className="h-7 rounded-none px-2 text-xs"
            >
              <FileCode2 className="size-3.5" aria-hidden="true" />
              Text
            </TabsTrigger>
            <TabsTrigger
              value="file"
              className="h-7 rounded-none px-2 text-xs"
            >
              <Database className="size-3.5" aria-hidden="true" />
              File
            </TabsTrigger>
            <TabsTrigger value="hex" className="h-7 rounded-none px-2 text-xs">
              <Hexagon className="size-3.5" aria-hidden="true" />
              Hex
            </TabsTrigger>
          </TabsList>
        </div>

        <TabsContent
          value="text"
          className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <TextPreviewViewer
            path={selectedEntry?.kind === "file" ? selectedEntry.path : null}
            emptyText={
              selectedEntry?.kind === "directory"
                ? "Select a file to preview text."
                : "No text preview available."
            }
          />
        </TabsContent>

        <TabsContent
          value="file"
          className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <ApplicationPreviewViewer
            filePreview={filePreview}
            isLoading={isLoading}
            selectedEntry={selectedEntry}
          />
        </TabsContent>

        <TabsContent
          value="hex"
          className="m-0 min-h-0 min-w-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
        >
          <HexPreviewViewer
            path={selectedEntry?.kind === "file" ? selectedEntry.path : null}
            emptyText={
              selectedEntry?.kind === "directory"
                ? "Select a file to preview hex."
                : "No hex preview available."
            }
          />
        </TabsContent>
      </Tabs>
    </section>
  );
}
