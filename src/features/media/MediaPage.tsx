import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  FixedSizeGrid,
  type GridChildComponentProps,
} from "react-window";
import {
  AlertCircle,
  Camera,
  Clapperboard,
  FileImage,
  Film,
  Images,
  RefreshCw,
  Video,
} from "lucide-react";
import ReactPlayer from "react-player";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useCases } from "@/features/cases/case-provider";
import { listDataSources } from "@/features/datasources/dataSourceRepository";
import type { DataSourceRecord } from "@/features/datasources/types";
import { listMediaGallery } from "@/features/media/mediaRepository";
import type { MediaGalleryResult, MediaItem } from "@/features/media/types";
import { cn } from "@/lib/utils";

type MediaGalleryProps = {
  detailTitle: string;
  emptyText: string;
  icon: typeof FileImage;
  isLoading: boolean;
  mediaType: "image" | "video";
  onSelectItem: (item: MediaItem) => void;
  selectedItem: MediaItem | null;
  tiles: MediaItem[];
  title: string;
};

type ElementSize = {
  height: number;
  width: number;
};

type VirtualizedMediaGridData = {
  columnCount: number;
  mediaType: "image" | "video";
  onSelectItem: (item: MediaItem) => void;
  selectedPath: string | null;
  tiles: MediaItem[];
};

const MEDIA_TILE_MIN_WIDTH = 144;
const MEDIA_TILE_GAP = 8;
const MEDIA_TILE_ASPECT_RATIO = 3 / 4;
const MEDIA_GRID_OVERSCAN_ROWS = 3;

export function MediaPage() {
  const { activeCase } = useCases();
  const [gallery, setGallery] = useState<MediaGalleryResult>({
    photos: [],
    videos: [],
  });
  const [selectedItem, setSelectedItem] = useState<MediaItem | null>(null);
  const [playerItem, setPlayerItem] = useState<MediaItem | null>(null);
  const [dataSources, setDataSources] = useState<DataSourceRecord[]>([]);
  const [selectedDataSourceId, setSelectedDataSourceId] = useState<string | null>(
    null,
  );
  const [isLoading, setIsLoading] = useState(false);
  const [isDataSourcesLoading, setIsDataSourcesLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);

  useEffect(() => {
    if (!activeCase) {
      setDataSources([]);
      setSelectedDataSourceId(null);
      return;
    }

    let isCurrent = true;

    setIsDataSourcesLoading(true);

    listDataSources(activeCase.databasePath, activeCase.id)
      .then((nextDataSources) => {
        if (!isCurrent) {
          return;
        }

        setDataSources(nextDataSources);
        setSelectedDataSourceId((currentId) => {
          if (
            currentId &&
            nextDataSources.some((dataSource) => dataSource.id === currentId)
          ) {
            return currentId;
          }

          return nextDataSources[0]?.id ?? null;
        });
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setError(getErrorMessage(caughtError));
        setDataSources([]);
        setSelectedDataSourceId(null);
      })
      .finally(() => {
        if (isCurrent) {
          setIsDataSourcesLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase]);

  useEffect(() => {
    if (!activeCase) {
      setGallery({ photos: [], videos: [] });
      setSelectedItem(null);
      setPlayerItem(null);
      setError(null);
      return;
    }

    if (!selectedDataSourceId) {
      setGallery({ photos: [], videos: [] });
      setSelectedItem(null);
      setPlayerItem(null);
      setError(null);
      setIsLoading(false);
      return;
    }

    let isCurrent = true;

    setIsLoading(true);
    setError(null);

    listMediaGallery(activeCase.databasePath, selectedDataSourceId)
      .then((nextGallery) => {
        if (!isCurrent) {
          return;
        }

        const mediaGallery = normalizeMediaGallery(nextGallery);

        setGallery(mediaGallery);
        setSelectedItem((currentItem) => {
          if (
            currentItem &&
            [...mediaGallery.photos, ...mediaGallery.videos].some(
              (item) => item.path === currentItem.path,
            )
          ) {
            return currentItem;
          }

          return mediaGallery.photos[0] ?? mediaGallery.videos[0] ?? null;
        });
        setPlayerItem((currentItem) => {
          if (
            currentItem &&
            mediaGallery.videos.some((item) => item.path === currentItem.path)
          ) {
            return currentItem;
          }

          return null;
        });
      })
      .catch((caughtError) => {
        if (!isCurrent) {
          return;
        }

        setError(getErrorMessage(caughtError));
        setGallery({ photos: [], videos: [] });
        setSelectedItem(null);
        setPlayerItem(null);
      })
      .finally(() => {
        if (isCurrent) {
          setIsLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase, refreshKey, selectedDataSourceId]);

  const totalMedia = gallery.photos.length + gallery.videos.length;
  const handleSelectItem = (item: MediaItem) => {
    setSelectedItem(item);

    if (item.mediaType === "video") {
      setPlayerItem(item);
    }
  };

  const selectedDataSource = useMemo(() => {
    return (
      dataSources.find((dataSource) => dataSource.id === selectedDataSourceId) ??
      null
    );
  }, [dataSources, selectedDataSourceId]);

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <div className="flex min-w-0 items-center gap-2">
          <Images className="size-3.5 text-muted-foreground" aria-hidden="true" />
          <h1 className="text-sm font-semibold">Media</h1>
          <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
            Built-in Rust scanner
          </Badge>
        </div>
        <Separator orientation="vertical" className="h-5" />
        <div className="flex min-w-0 items-center gap-2 text-[11px] text-muted-foreground">
          <span>Photos: {gallery.photos.length.toLocaleString()}</span>
          <span>Videos: {gallery.videos.length.toLocaleString()}</span>
          <span>Items: {totalMedia.toLocaleString()}</span>
        </div>
        <Separator orientation="vertical" className="h-5" />
        <div className="flex min-w-0 items-center gap-1">
          <span className="shrink-0 text-[11px] text-muted-foreground">
            Datasource
          </span>
          <div className="flex min-w-0 items-center gap-1 overflow-hidden">
            {dataSources.map((dataSource) => (
              <Button
                key={dataSource.id}
                type="button"
                variant={
                  dataSource.id === selectedDataSourceId ? "secondary" : "ghost"
                }
                size="xs"
                className="h-7 max-w-40 rounded-sm px-2 text-xs"
                disabled={isLoading}
                title={dataSource.name}
                onClick={() => setSelectedDataSourceId(dataSource.id)}
              >
                <span className="truncate">{dataSource.name}</span>
              </Button>
            ))}
            {dataSources.length === 0 && (
              <span className="text-[11px] text-muted-foreground">
                {isDataSourcesLoading ? "Loading" : "None"}
              </span>
            )}
          </div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="ml-auto h-7 rounded-sm px-2 text-xs"
          disabled={!activeCase || isLoading}
          onClick={() => setRefreshKey((key) => key + 1)}
        >
          <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
          Refresh
        </Button>
      </section>

      {error && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{error}</span>
        </section>
      )}

      <Tabs
        defaultValue="photos"
        className="flex min-h-0 flex-1 flex-col gap-0"
      >
        <section className="flex h-8 shrink-0 items-center justify-between border-b px-2">
          <TabsList
            variant="line"
            className="h-7 rounded-none p-0"
            aria-label="Media gallery"
          >
            <TabsTrigger value="photos" className="h-7 rounded-none px-2 text-xs">
              <Camera className="size-3.5" aria-hidden="true" />
              Photos
            </TabsTrigger>
            <TabsTrigger value="videos" className="h-7 rounded-none px-2 text-xs">
              <Film className="size-3.5" aria-hidden="true" />
              Videos
            </TabsTrigger>
          </TabsList>
          <div className="truncate text-[11px] text-muted-foreground">
            {activeCase
              ? isLoading
                ? "Loading media gallery..."
                : `${totalMedia.toLocaleString()} media files${
                    selectedDataSource ? ` in ${selectedDataSource.name}` : ""
                  }`
              : "Open or create a case to view media"}
          </div>
        </section>

        <TabsContent value="photos" className="m-0 min-h-0 flex-1">
          <MediaGallery
            title="Photo Gallery"
            icon={FileImage}
            tiles={gallery.photos}
            detailTitle="Image Details"
            emptyText="No images found yet. Run the Image Metadata plugin from Plugins."
            isLoading={isLoading}
            mediaType="image"
            selectedItem={selectedItem?.mediaType === "image" ? selectedItem : null}
            onSelectItem={handleSelectItem}
          />
        </TabsContent>
        <TabsContent value="videos" className="m-0 min-h-0 flex-1">
          <MediaGallery
            title="Video Gallery"
            icon={Video}
            tiles={gallery.videos}
            detailTitle="Video Details"
            emptyText="No videos found yet. Run the Image Metadata plugin from Plugins."
            isLoading={isLoading}
            mediaType="video"
            selectedItem={selectedItem?.mediaType === "video" ? selectedItem : null}
            onSelectItem={handleSelectItem}
          />
        </TabsContent>
      </Tabs>

      <VideoPlayerSheet item={playerItem} onOpenChange={setPlayerItem} />
    </div>
  );
}

function MediaGallery({
  detailTitle,
  emptyText,
  icon: Icon,
  isLoading,
  mediaType,
  onSelectItem,
  selectedItem,
  tiles,
  title,
}: MediaGalleryProps) {
  return (
    <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_16rem]">
      <section className="flex min-h-0 min-w-0 flex-col border-r">
        <div className="flex h-8 shrink-0 items-center gap-2 border-b px-2">
          <Icon className="size-3.5 text-muted-foreground" aria-hidden="true" />
          <h2 className="text-xs font-medium">{title}</h2>
          <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
            {tiles.length.toLocaleString()}
          </Badge>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden p-2">
          {tiles.length > 0 ? (
            <VirtualizedMediaGrid
              mediaType={mediaType}
              onSelectItem={onSelectItem}
              selectedPath={selectedItem?.path ?? null}
              tiles={tiles}
            />
          ) : (
            <div className="flex h-full min-h-24 items-center justify-center text-xs text-muted-foreground">
              {isLoading ? "Scanning media..." : emptyText}
            </div>
          )}
        </div>
      </section>

      <MediaDetailsPanel title={detailTitle} item={selectedItem} />
    </div>
  );
}

function VirtualizedMediaGrid({
  mediaType,
  onSelectItem,
  selectedPath,
  tiles,
}: {
  mediaType: "image" | "video";
  onSelectItem: (item: MediaItem) => void;
  selectedPath: string | null;
  tiles: MediaItem[];
}) {
  const [containerRef, size] = useElementSize<HTMLDivElement>();
  const columnCount = Math.max(
    1,
    Math.floor(Math.max(size.width, MEDIA_TILE_MIN_WIDTH) / MEDIA_TILE_MIN_WIDTH),
  );
  const columnWidth = Math.max(
    MEDIA_TILE_MIN_WIDTH,
    Math.floor(Math.max(size.width, MEDIA_TILE_MIN_WIDTH) / columnCount),
  );
  const rowHeight =
    Math.round((columnWidth - MEDIA_TILE_GAP) * MEDIA_TILE_ASPECT_RATIO) +
    MEDIA_TILE_GAP;
  const rowCount = Math.ceil(tiles.length / columnCount);
  const gridData: VirtualizedMediaGridData = {
    columnCount,
    mediaType,
    onSelectItem,
    selectedPath,
    tiles,
  };

  return (
    <div ref={containerRef} className="h-full min-h-0 w-full overflow-hidden">
      {size.width > 0 && size.height > 0 ? (
        <FixedSizeGrid<VirtualizedMediaGridData>
          columnCount={columnCount}
          columnWidth={columnWidth}
          height={size.height}
          itemData={gridData}
          overscanColumnCount={1}
          overscanRowCount={MEDIA_GRID_OVERSCAN_ROWS}
          rowCount={rowCount}
          rowHeight={rowHeight}
          width={size.width}
        >
          {MediaGridCell}
        </FixedSizeGrid>
      ) : null}
    </div>
  );
}

function MediaGridCell({
  columnIndex,
  data,
  rowIndex,
  style,
}: GridChildComponentProps<VirtualizedMediaGridData>) {
  const itemIndex = rowIndex * data.columnCount + columnIndex;
  const tile = data.tiles[itemIndex];

  if (!tile) {
    return null;
  }

  return (
    <div style={style} className="p-1">
      <MediaTilePreview
        className="h-full w-full"
        isSelected={data.selectedPath === tile.path}
        mediaType={data.mediaType}
        tile={tile}
        onSelectItem={data.onSelectItem}
      />
    </div>
  );
}

function MediaTilePreview({
  className,
  isSelected,
  mediaType,
  onSelectItem,
  tile,
}: {
  className?: string;
  isSelected: boolean;
  mediaType: "image" | "video";
  onSelectItem: (item: MediaItem) => void;
  tile: MediaItem;
}) {
  const source = convertFileSrc(tile.thumbnailPath || tile.mediaPath);
  const videoSource = `${source}#t=0.1`;

  return (
    <button
      type="button"
      className={cn(
        "overflow-hidden rounded-sm border bg-muted text-left",
        isSelected && "ring-2 ring-ring",
        className,
      )}
      aria-label={tile.name}
      title={tile.path}
      onClick={() => onSelectItem(tile)}
    >
      {mediaType === "image" ? (
        <img
          src={source}
          alt={tile.name}
          className="h-full w-full object-cover"
          draggable={false}
          loading="lazy"
        />
      ) : (
        <video
          src={videoSource}
          className="h-full w-full object-cover"
          muted
          playsInline
          preload="metadata"
        />
      )}
    </button>
  );
}

function VideoPlayerSheet({
  item,
  onOpenChange,
}: {
  item: MediaItem | null;
  onOpenChange: (item: MediaItem | null) => void;
}) {
  const source = item ? convertFileSrc(item.mediaPath) : "";

  return (
    <Sheet
      open={Boolean(item)}
      onOpenChange={(isOpen) => {
        if (!isOpen) {
          onOpenChange(null);
        }
      }}
    >
      <SheetContent
        side="bottom"
        className="h-[min(72vh,42rem)] gap-0 p-0"
        aria-describedby="video-player-description"
      >
        <SheetHeader className="h-10 shrink-0 flex-row items-center gap-2 border-b px-2 py-0">
          <Video className="size-3.5 text-muted-foreground" aria-hidden="true" />
          <div className="min-w-0 flex-1">
            <SheetTitle className="truncate text-xs font-medium">
              {item?.name ?? "Video Player"}
            </SheetTitle>
            <SheetDescription
              id="video-player-description"
              className="truncate text-[11px]"
            >
              {item?.path ?? "No video selected"}
            </SheetDescription>
          </div>
        </SheetHeader>
        <div className="min-h-0 flex-1 bg-black">
          {item ? (
            <ReactPlayer
              src={source}
              controls
              playing
              width="100%"
              height="100%"
              style={{ backgroundColor: "black" }}
            />
          ) : null}
        </div>
      </SheetContent>
    </Sheet>
  );
}

function useElementSize<T extends HTMLElement>() {
  const ref = useRef<T | null>(null);
  const [size, setSize] = useState<ElementSize>({ height: 0, width: 0 });

  useLayoutEffect(() => {
    const observedElement = ref.current;

    if (!observedElement) {
      return;
    }

    const resizeObserver = new ResizeObserver(([entry]) => {
      setSize({
        height: entry.contentRect.height,
        width: entry.contentRect.width,
      });
    });

    setSize({
      height: observedElement.clientHeight,
      width: observedElement.clientWidth,
    });
    resizeObserver.observe(observedElement);

    return () => resizeObserver.disconnect();
  }, []);

  return [ref, size] as const;
}

function MediaDetailsPanel({
  item,
  title,
}: {
  item: MediaItem | null;
  title: string;
}) {
  return (
    <aside className="flex min-h-0 flex-col">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b px-2">
        <Clapperboard
          className="size-3.5 text-muted-foreground"
          aria-hidden="true"
        />
        <h2 className="text-xs font-medium">{title}</h2>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-2">
        {item ? (
          <div className="grid gap-2 text-xs">
            <DetailRow label="Name" value={item.name} />
            <DetailRow label="Format" value={item.format} />
            <DetailRow
              label="Dimensions"
              value={
                item.width && item.height ? `${item.width} x ${item.height}` : "-"
              }
            />
            <DetailRow label="Duration" value={formatDuration(item.durationMs)} />
            <DetailRow label="Size" value={formatBytes(item.size)} />
            <DetailRow label="Modified" value={formatModifiedTime(item.modifiedMs)} />
            <DetailRow label="Path" value={item.path} />
            {item.metadata.map((field, index) => (
              <DetailRow
                key={`${index}:${field.label}`}
                label={field.label}
                value={field.value}
              />
            ))}
          </div>
        ) : (
          <div className="text-xs text-muted-foreground">
            Select a media item to inspect metadata.
          </div>
        )}
      </div>
    </aside>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[7rem_minmax(0,1fr)] gap-2 border-b pb-1">
      <div className="min-w-0 break-words text-[11px] text-muted-foreground">
        {label}
      </div>
      <div
        className="min-w-0 whitespace-pre-wrap break-words font-mono text-[11px]"
        title={value}
      >
        {value || "-"}
      </div>
    </div>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${value.toFixed(value >= 10 ? 1 : 2)} ${units[unitIndex]}`;
}

function formatDuration(durationMs: number | null | undefined) {
  if (!durationMs) {
    return "-";
  }

  const totalSeconds = Math.round(durationMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, "0")}:${seconds
      .toString()
      .padStart(2, "0")}`;
  }

  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

function formatModifiedTime(modifiedMs: number | null | undefined) {
  if (!modifiedMs) {
    return "-";
  }

  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(modifiedMs));
}

function normalizeMediaGallery(gallery: MediaGalleryResult): MediaGalleryResult {
  return {
    photos: Array.isArray(gallery.photos) ? gallery.photos : [],
    videos: Array.isArray(gallery.videos) ? gallery.videos : [],
    scannedFiles: gallery.scannedFiles,
  };
}

function getErrorMessage(caughtError: unknown) {
  return caughtError instanceof Error ? caughtError.message : String(caughtError);
}
