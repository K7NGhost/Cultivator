import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  FixedSizeGrid,
  type GridChildComponentProps,
} from "react-window";
import {
  AlertCircle,
  Camera,
  Clapperboard,
  FileImage,
  Expand,
  Film,
  FolderSearch,
  Images,
  Layers,
  RefreshCw,
  Video,
} from "lucide-react";
import ReactPlayer from "react-player";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useCases } from "@/features/cases";
import {
  listDataSources,
  subscribeToDataSourcesChanged,
} from "@/features/datasources";
import type { DataSourceRecord } from "@/features/datasources";
import { MediaImagePreview } from "../components/MediaImagePreview";
import { DirectorySearch } from "../components/DirectorySearch";
import { HashsetPanel } from "../components/HashsetPanel";
import { listHashsetHitMediaPage, type HashsetFilter } from "../services/hashsetRepository";
import { ThumbnailPreparationStatus } from "../components/ThumbnailPreparationStatus";
import { MediaVideoPreview } from "../components/MediaVideoPreview";
import {
  type FileViewMediaPage,
  listClassifiedFileViewMediaPage,
  listMediaGalleryPage,
  loadMediaDetails,
  prepareCaseThumbnails,
  setMediaClassification,
} from "../services/mediaRepository";
import type { MediaGalleryResult, MediaItem } from "../model/types";
import { VIC_CATEGORIES, vicClassification, classificationTextColor, matchesVicFilter, type ClassificationDefinition, type VicCategory, type VicFilterValue } from "../model/vicClassification";
import { useCustomClassifications } from "../services/useCustomClassifications";
import type { CustomClassificationActions } from "../components/CustomClassificationManager";
import { VicClassificationControls, VicFilterSidebar, type VisualGalleryControls } from "../components/VicClassification";
import { nextReviewImage } from "../services/nextReviewImage";
import { GALLERY_TILE_SIZES, galleryTileLayout, parseGalleryTileSize, readGalleryTileSize, saveGalleryTileSize, type GalleryTileSize } from "../model/galleryTileSize";
import { cn } from "@/lib/utils";
import { VirtualMediaCache } from "../services/virtualMediaCache";
import { galleryScrollAnchor } from "../services/galleryScrollAnchor";
import { usePerformanceSettings, recordCacheBytes, thumbnailCacheBytes, thumbnailConcurrency } from "../../settings/performanceSettings";
import { useGalleryActive } from "../services/useGalleryActive";
import { clearVideoThumbnails } from "../utils/videoThumbnails";
import { GalleryPrefetch, galleryPrefetchRange } from "../services/galleryPrefetch";
import { clearPreparedThumbnails, prepareThumbnail, useThumbnailEpoch } from "../services/preparedThumbnails";

const MediaImageViewer = lazy(() => import("../components/MediaImageViewer"));

type GalleryClassification = {
  showFaceBoxes: boolean;
  onShowFaceBoxesChange: (value: boolean) => void;
  visual?: VisualGalleryControls;
  custom: CustomClassificationActions;
  focusTarget?: { path: string; index: number } | null;
  filter: VicFilterValue[];
  counts: number[];
  onFilterChange: (filter: VicFilterValue[]) => void;
  onClassify: (item: MediaItem, category: VicCategory | null, advanceFromIndex?: number) => void;
  saving: boolean;
};
type MediaGalleryProps = {
  tileSize: GalleryTileSize;
  onTileSizeChange: (size: GalleryTileSize) => void;
  classification?: GalleryClassification;
  directorySearch?: ReactNode;
  caseDatabasePath: string | null;
  detailTitle: string;
  emptyText: string;
  icon: typeof FileImage;
  isLoading: boolean;
  mediaType: "image" | "video";
  fetchPage: (type: "image" | "video", offset: number) => Promise<FileViewMediaPage>;
  onSelectItem: (item: MediaItem) => void;
  onOpenImage?: (item: MediaItem) => void;
  selectedItem: MediaItem | null;
  tiles: MediaItem[];
  title: string;
  totalCount?: number;
};

type ElementSize = {
  height: number;
  width: number;
};

type VirtualizedMediaGridData = {
  showFaceBoxes: boolean;
  thumbnailEdge: number;
  customCategories?: ClassificationDefinition[];
  onClassify?: GalleryClassification["onClassify"];
  classificationSaving?: boolean;
  caseDatabasePath: string | null;
  columnCount: number;
  mediaType: "image" | "video";
  onSelectItem: (item: MediaItem) => void;
  onOpenImage?: (item: MediaItem) => void;
  selectedPath: string | null;
  cache: VirtualMediaCache;
};

const MEDIA_TILE_GAP = 8;
const MEDIA_TILE_ASPECT_RATIO = 3 / 4;
type MediaSource = "plugin" | "file-view";

export function MediaPage() {
  const [showFaceBoxes, setShowFaceBoxes] = useState(() => {
    try { return localStorage.getItem("cultivator.media.showFaceBoxes") !== "false"; } catch { return true; }
  });
  const handleShowFaceBoxesChange = (visible: boolean) => {
    setShowFaceBoxes(visible);
    try { localStorage.setItem("cultivator.media.showFaceBoxes", String(visible)); } catch { /* Keep the session preference. */ }
  };
  const [tileSize, setTileSize] = useState<GalleryTileSize>(readGalleryTileSize);
  const handleTileSizeChange = (size: GalleryTileSize) => { setTileSize(size); saveGalleryTileSize(size); };
  const galleryActive = useGalleryActive();
  useEffect(() => {
    if (!galleryActive) { clearPreparedThumbnails(); clearVideoThumbnails(); }
  }, [galleryActive]);
  const { activeCase } = useCases();
  const [gallery, setGallery] = useState<MediaGalleryResult>({
    photos: [],
    videos: [],
  });
  const [selectedItem, setSelectedItem] = useState<MediaItem | null>(null);
  const [playerItem, setPlayerItem] = useState<MediaItem | null>(null);
  const [viewer, setViewer] = useState<{ item: MediaItem; context: string } | null>(null);
  const [dataSources, setDataSources] = useState<DataSourceRecord[]>([]);
  const [selectedDataSourceId, setSelectedDataSourceId] = useState<string | null>(
    null,
  );
  const [mediaSource, setMediaSource] = useState<MediaSource>("plugin");
  const [classificationFilter, setClassificationFilter] = useState<VicFilterValue[]>([]);
  const [classificationRevision, setClassificationRevision] = useState(0);
  const [classificationSaving, setClassificationSaving] = useState(false);
  const [reviewFocusTarget, setReviewFocusTarget] = useState<GalleryClassification["focusTarget"]>(null);
  const selectedItemRef = useRef(selectedItem);
  selectedItemRef.current = selectedItem;
  const classificationContext = JSON.stringify([activeCase?.databasePath, selectedDataSourceId, mediaSource]);
  useEffect(() => { setViewer(null); }, [classificationContext, galleryActive]);
  const [directorySelection, setDirectorySelection] = useState<{ context: string; path: string } | null>(null);
  const directoryFilter = directorySelection?.context === classificationContext ? directorySelection.path : null;
  const [hashsetSelection, setHashsetSelection] = useState<{ context: string; filter: HashsetFilter } | null>(null);
  const hashsetFilter = hashsetSelection?.context === classificationContext ? hashsetSelection.filter : null;
  const [skinMin, setSkinMin] = useState<number | null>(null);
  const [collapseSimilar, setCollapseSimilar] = useState(true);
  const [stackSelection, setStackSelection] = useState<{ context: string; id: string } | null>(null);
  const stackId = stackSelection?.context === classificationContext ? stackSelection.id : null;
  const visualOptions = useMemo(() => mediaSource === "plugin" && !hashsetFilter
    ? { skinMin, collapseSimilar, stackId } : undefined, [mediaSource, !!hashsetFilter, skinMin, collapseSimilar, stackId]);
  const sidebarContext = JSON.stringify([classificationContext, hashsetFilter?.runId]);
  const emptySidebarCounts = useMemo(() => Array(mediaSource === "plugin" && !hashsetFilter ? 276 : 257).fill(0) as number[], [mediaSource, !!hashsetFilter]);
  const [sidebarCounts, setSidebarCounts] = useState<{ context: string; counts: number[] } | null>(null);
  useEffect(() => {
    if (mediaSource !== "plugin" || hashsetFilter) {
      setClassificationFilter(current => current.some(value => value >= 256) ? current.filter(value => value < 256) : current);
    }
  }, [mediaSource, hashsetFilter]);
  const classificationContextRef = useRef(classificationContext);
  classificationContextRef.current = classificationContext;
  const galleryScopeRef = useRef<string | undefined>(undefined);
  useEffect(() => { setClassificationSaving(false); }, [classificationContext]);
  useEffect(() => { setReviewFocusTarget(null); }, [classificationContext, classificationFilter, directoryFilter, hashsetFilter?.runId, visualOptions]);
  const [isLoading, setIsLoading] = useState(false);
  const [isDataSourcesLoading, setIsDataSourcesLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const handleDefinitionsChanged = useCallback((deletedCategory?: number) => {
    if (deletedCategory !== undefined) {
      setSelectedItem((item) => item?.vicCategory === deletedCategory ? { ...item, vicCategory: null } : item);
      setGallery((current) => ({ ...current, photos: current.photos.map((item) => item.vicCategory === deletedCategory ? { ...item, vicCategory: null } : item) }));
      setClassificationFilter((current) => current.includes(deletedCategory) ? current.filter((category) => category !== deletedCategory) : current);
    }
    setClassificationRevision((revision) => revision + 1);
  }, []);
  const customClassifications = useCustomClassifications(activeCase?.databasePath ?? null, handleDefinitionsChanged, setError);
  useEffect(() => {
    setClassificationFilter((current) => current.some((category) => category > 4) ? current.filter((category) => category <= 4) : current);
  }, [activeCase?.databasePath]);
  const [refreshKey, setRefreshKey] = useState(0);
  useEffect(() => {
    // A refresh or source change starts a new indexed-evidence snapshot. Native
    // requests will revalidate the source fingerprint before repopulating RAM.
    clearPreparedThumbnails();
  }, [activeCase?.databasePath, selectedDataSourceId, mediaSource, refreshKey, directoryFilter, hashsetFilter?.runId]);
  const [fileViewPages, setFileViewPages] = useState<{
    image: FileViewMediaPage | null;
    video: FileViewMediaPage | null;
  }>({ image: null, video: null });
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
  }, [activeCase, refreshKey]);

  useEffect(() => {
    if (!activeCase) {
      return;
    }

    return subscribeToDataSourcesChanged((caseId) => {
      if (caseId === activeCase.id) {
        setRefreshKey((key) => key + 1);
      }
    });
  }, [activeCase]);

  useEffect(() => {
    if (!activeCase) {
      setGallery({ photos: [], videos: [] });
      setSelectedItem(null);
      setPlayerItem(null);
      setError(null);
      setFileViewPages({ image: null, video: null });
      return;
    }

    if (!selectedDataSourceId) {
      setGallery({ photos: [], videos: [] });
      setFileViewPages({ image: null, video: null });
      setSelectedItem(null);
      setPlayerItem(null);
      setError(null);
      setIsLoading(false);
      return;
    }

    let isCurrent = true;
    const scope = JSON.stringify([classificationContext, classificationFilter, directoryFilter, hashsetFilter?.runId, visualOptions]);
    if (galleryScopeRef.current !== scope) {
      setGallery({ photos: [], videos: [] });
      setFileViewPages({ image: null, video: null });
      setSelectedItem(null);
      galleryScopeRef.current = scope;
    }

    setIsLoading(true);
    setError(null);

    const selectedDataSource = dataSources.find(
      (dataSource) => dataSource.id === selectedDataSourceId,
    );
    const getPage = (type: "image" | "video") => hashsetFilter
      ? listHashsetHitMediaPage(activeCase.databasePath, hashsetFilter.runId, type, 0, type === "image" ? classificationFilter : [], directoryFilter)
      : mediaSource === "plugin"
      ? listMediaGalleryPage(activeCase.databasePath, selectedDataSourceId, type, 0, type === "image" ? classificationFilter : [], directoryFilter, visualOptions)
      : listClassifiedFileViewMediaPage(activeCase.databasePath, selectedDataSource?.paths ?? [], type, 0, type === "image" ? classificationFilter : [], directoryFilter);
    const galleryRequest = Promise.all([getPage("image"), getPage("video")])
      .then(([image, video]) => ({
        gallery: { photos: image.items, videos: video.items }, pages: { image, video },
      }));

    galleryRequest
      .then(({ gallery: nextGallery, pages }) => {
        if (!isCurrent) {
          return;
        }

        const mediaGallery = normalizeMediaGallery(nextGallery);

        setGallery(mediaGallery);
        setFileViewPages(pages);
        // Page resets belong to the gallery. Keep sidebar groups, counts and
        // native dropdown DOM intact until fresh counts arrive.
        const counts = pages.image.classificationCounts ?? emptySidebarCounts;
        setSidebarCounts(current => current?.context === sidebarContext
          && current.counts.length === counts.length && current.counts.every((count, index) => count === counts[index])
          ? current : { context: sidebarContext, counts });
        setSelectedItem((currentItem) => {
          if (
            currentItem &&
            (currentItem.mediaType === "video" || matchesVicFilter(currentItem.vicCategory, classificationFilter, currentItem.faceCount, currentItem.clipClassifications, currentItem.nsfwScore))
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
        setFileViewPages({ image: null, video: null });
      })
      .finally(() => {
        if (isCurrent) {
          setIsLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase, dataSources, mediaSource, refreshKey, selectedDataSourceId, classificationContext, classificationFilter, classificationRevision, directoryFilter, hashsetFilter, sidebarContext, emptySidebarCounts, visualOptions]);

  useEffect(() => {
    if (!activeCase || !selectedDataSourceId || mediaSource !== "plugin" ||
        !selectedItem || selectedItem.metadataLoaded) return;
    let current = true;
    loadMediaDetails(activeCase.databasePath, selectedDataSourceId, selectedItem.path)
      .then((item) => { if (current) setSelectedItem(previous => previous?.path === item.path
        ? { ...item, stackSize: previous.stackSize, metadata: [...item.metadata, ...previous.metadata.filter(field => field.label === "Hashset hit")] } : previous); })
      .catch((error) => { if (current) setError(getErrorMessage(error)); });
    return () => { current = false; };
  }, [activeCase, selectedDataSourceId, mediaSource, selectedItem]);

  const photoCount = fileViewPages.image?.totalCount ?? 0;
  const videoCount = fileViewPages.video?.totalCount ?? 0;
  const totalMedia = photoCount + videoCount;
  const handleSelectItem = (item: MediaItem) => {
    selectedItemRef.current = item;
    setReviewFocusTarget(null);
    setSelectedItem(item);

    if (item.mediaType === "video") {
      setPlayerItem(item);
    }
  };
  const fetchPage = useCallback((type: "image" | "video", offset: number) => {
    const datasource = dataSources.find((source) => source.id === selectedDataSourceId);
    if (!activeCase || !datasource) return Promise.reject(new Error("No datasource selected."));
    return hashsetFilter
      ? listHashsetHitMediaPage(activeCase.databasePath, hashsetFilter.runId, type, offset, type === "image" ? classificationFilter : [], directoryFilter)
      : mediaSource === "plugin"
      ? listMediaGalleryPage(activeCase.databasePath, datasource.id, type, offset, type === "image" ? classificationFilter : [], directoryFilter, visualOptions)
      : listClassifiedFileViewMediaPage(activeCase.databasePath, datasource.paths, type, offset, type === "image" ? classificationFilter : [], directoryFilter);
  }, [activeCase, dataSources, selectedDataSourceId, mediaSource, classificationFilter, classificationRevision, directoryFilter, hashsetFilter, visualOptions]);

  async function handleClassify(item: MediaItem, category: VicCategory | null, advanceFromIndex?: number) {
    if (!activeCase || item.mediaType !== "image" || classificationSaving || isLoading || customClassifications.busy || !customClassifications.ready) return;
    const context = classificationContext;
    const reviewScope = galleryScopeRef.current;
    setClassificationSaving(true);
    setError(null);
    try {
      await setMediaClassification(activeCase.databasePath, item.path, category);
      if (classificationContextRef.current !== context) return;
      setSelectedItem((current) => current?.path === item.path ? { ...current, vicCategory: category } : current);
      setGallery((current) => ({ ...current, photos: current.photos.map((photo) => photo.path === item.path ? { ...photo, vicCategory: category } : photo) }));
      try {
        if (advanceFromIndex !== undefined && galleryScopeRef.current === reviewScope && selectedItemRef.current?.path === item.path) {
          const next = await nextReviewImage(advanceFromIndex, matchesVicFilter(category, classificationFilter, item.faceCount, item.clipClassifications, item.nsfwScore),
            (offset) => fetchPage("image", offset));
          if (classificationContextRef.current !== context || galleryScopeRef.current !== reviewScope || selectedItemRef.current?.path !== item.path) return;
          setSelectedItem(next?.item ?? null);
          setReviewFocusTarget(next ? { path: next.item.path, index: next.index } : null);
        }
      } finally {
        if (classificationContextRef.current === context) setClassificationRevision((revision) => revision + 1);
      }
    } catch (error) {
      if (classificationContextRef.current === context) setError(getErrorMessage(error));
    } finally {
      if (classificationContextRef.current === context) setClassificationSaving(false);
    }
  }

  const selectedDataSource = useMemo(() => {
    return (
      dataSources.find((dataSource) => dataSource.id === selectedDataSourceId) ??
      null
    );
  }, [dataSources, selectedDataSourceId]);

  const mediaFilters = (type: "image" | "video") => activeCase && selectedDataSource && <>
    <DirectorySearch key={`directories:${classificationContext}`} caseDatabasePath={activeCase.databasePath}
      datasourceId={selectedDataSource.id} roots={selectedDataSource.paths} indexed={mediaSource === "plugin"} mediaType={type}
      selectedDirectory={directoryFilter} onSelect={path => setDirectorySelection(path ? { context: classificationContext, path } : null)} revision={refreshKey} />
    <HashsetPanel key={`hashsets:${classificationContext}`} caseDatabasePath={activeCase.databasePath}
      datasourceId={selectedDataSource.id} indexed={mediaSource === "plugin"} selected={hashsetFilter}
      onSelect={filter => setHashsetSelection(filter ? { context: classificationContext, filter } : null)} />
  </>;

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <div className="flex min-w-0 items-center gap-2">
          <Images className="size-3.5 text-muted-foreground" aria-hidden="true" />
          <h1 className="text-sm font-semibold">Media</h1>
          <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
            {mediaSource === "plugin" ? "Byte detection" : "Extension filter"}
          </Badge>
        </div>
        <Separator orientation="vertical" className="h-5" />
        <div className="flex min-w-0 items-center gap-2 text-[11px] text-muted-foreground">
          <span>Photos: {photoCount.toLocaleString()}</span>
          <span>Videos: {videoCount.toLocaleString()}</span>
          <span>Items: {totalMedia.toLocaleString()}</span>
        </div>
        <Separator orientation="vertical" className="h-5" />
        <div className="flex shrink-0 items-center gap-1">
          <span className="text-[11px] text-muted-foreground">Source</span>
          <Button
            type="button"
            variant={mediaSource === "plugin" ? "secondary" : "ghost"}
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={isLoading}
            onClick={() => setMediaSource("plugin")}
          >
            Plugin
          </Button>
          <Button
            type="button"
            variant={mediaSource === "file-view" ? "secondary" : "ghost"}
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={isLoading}
            onClick={() => setMediaSource("file-view")}
          >
            File View
          </Button>
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
        <ThumbnailPreparationStatus caseDatabasePath={activeCase?.databasePath ?? null} />
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="ml-auto h-7 rounded-sm px-2 text-xs"
          disabled={!activeCase || isLoading}
          onClick={() => {
            if (activeCase) void prepareCaseThumbnails(activeCase.databasePath, { rescan: true })
              .then(() => setRefreshKey(key => key + 1))
              .catch((error: unknown) => setError(getErrorMessage(error)));
          }}
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
            directorySearch={mediaFilters("image")}
            tileSize={tileSize}
            onTileSizeChange={handleTileSizeChange}
            classification={{
              showFaceBoxes, onShowFaceBoxesChange: handleShowFaceBoxesChange,
              custom: { categories: customClassifications.categories, disabled: !customClassifications.ready || customClassifications.busy,
                onSave: customClassifications.onSave, onDelete: customClassifications.onDelete },
              focusTarget: reviewFocusTarget,
              filter: classificationFilter,
              counts: sidebarCounts?.context === sidebarContext ? sidebarCounts.counts : emptySidebarCounts,
              onFilterChange: setClassificationFilter,
              visual: visualOptions ? {
                ...visualOptions, onSkinMinChange: setSkinMin, onCollapseSimilarChange: setCollapseSimilar,
                onOpenStack: id => setStackSelection({ context: classificationContext, id }),
                onClearStack: () => setStackSelection(null),
              } : undefined,
              onClassify: (item, category, advanceFromIndex) => { void handleClassify(item, category, advanceFromIndex); },
              saving: classificationSaving || isLoading || customClassifications.busy || !customClassifications.ready,
            }}
            caseDatabasePath={activeCase?.databasePath ?? null}
            title="Photo Gallery"
            icon={FileImage}
            tiles={gallery.photos}
            detailTitle="Image Details"
            emptyText={
              hashsetFilter ? "No hashset image hits match the current filters." : directoryFilter ? "No images match the filters in this directory." : classificationFilter.length || skinMin !== null || stackId ? "No images match the current filters." : mediaSource === "plugin"
                ? "No byte-detected images found. Run the Image Metadata plugin from Plugins."
                : "No files with image extensions were found in this datasource."
            }
            isLoading={isLoading}
            mediaType="image"
            fetchPage={fetchPage}
            selectedItem={selectedItem?.mediaType === "image" ? selectedItem : null}
            onSelectItem={handleSelectItem}
            onOpenImage={item => setViewer({ item, context: classificationContext })}
            totalCount={fileViewPages.image?.totalCount}
          />
        </TabsContent>
        <TabsContent value="videos" className="m-0 min-h-0 flex-1">
          <MediaGallery
            directorySearch={mediaFilters("video")}
            tileSize={tileSize}
            onTileSizeChange={handleTileSizeChange}
            caseDatabasePath={activeCase?.databasePath ?? null}
            title="Video Gallery"
            icon={Video}
            tiles={gallery.videos}
            detailTitle="Video Details"
            emptyText={
              hashsetFilter ? "No hashset video hits match the current filters." : directoryFilter ? "No videos directly in this directory." : mediaSource === "plugin"
                ? "No byte-detected videos found. Run the Image Metadata plugin from Plugins."
                : "No files with video extensions were found in this datasource."
            }
            isLoading={isLoading}
            mediaType="video"
            fetchPage={fetchPage}
            selectedItem={selectedItem?.mediaType === "video" ? selectedItem : null}
            onSelectItem={handleSelectItem}
            totalCount={fileViewPages.video?.totalCount}
          />
        </TabsContent>
      </Tabs>

      {galleryActive && viewer?.context === classificationContext && <Suspense fallback={null}>
        <MediaImageViewer key={viewer.item.path} item={viewer.item} onClose={() => setViewer(null)} />
      </Suspense>}
      <VideoPlayerSheet item={galleryActive ? playerItem : null} onOpenChange={setPlayerItem} />
    </div>
  );
}

function MediaGallery({
  tileSize,
  onTileSizeChange,
  classification,
  directorySearch,
  caseDatabasePath,
  detailTitle,
  emptyText,
  icon: Icon,
  isLoading,
  mediaType,
  fetchPage,
  onSelectItem,
  onOpenImage,
  selectedItem,
  tiles,
  title,
  totalCount,
}: MediaGalleryProps) {
  return (
    <div className={cn("grid h-full min-h-0", classification || directorySearch ? "grid-cols-[12rem_minmax(0,1fr)_16rem]" : "grid-cols-[minmax(0,1fr)_16rem]")}>
      {classification && <VicFilterSidebar key={caseDatabasePath} filter={classification.filter} counts={classification.counts}
        showFaceBoxes={classification.showFaceBoxes} onShowFaceBoxesChange={classification.onShowFaceBoxesChange}
        onChange={classification.onFilterChange} disabled={classification.custom.disabled} custom={classification.custom} directorySearch={directorySearch} visual={classification.visual} />}
      {!classification && directorySearch && <aside className="flex min-h-0 flex-col overflow-auto border-r bg-muted/20" aria-label="Video filters">
        <div className="flex h-8 shrink-0 items-center border-b px-2 text-xs font-medium">Filters</div>{directorySearch}
      </aside>}
      <section className="flex min-h-0 min-w-0 flex-col border-r">
        <div className="flex h-8 shrink-0 items-center gap-2 border-b px-2">
          <Icon className="size-3.5 text-muted-foreground" aria-hidden="true" />
          <h2 className="text-xs font-medium">{title}</h2>
          <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
            {(totalCount ?? tiles.length).toLocaleString()}{classification?.visual?.collapseSimilar && !classification.visual.stackId ? " groups" : ""}
          </Badge>
          {onOpenImage && <Button size="icon-xs" variant="ghost" title="Open image" aria-label="Open selected image"
            disabled={!selectedItem} onClick={() => { if (selectedItem) onOpenImage(selectedItem); }}><Expand /></Button>}
          <label className="ml-auto flex shrink-0 items-center gap-1 text-[11px]">
            Tile size
            <select aria-label={`${title} tile size`} value={tileSize}
              className="h-6 rounded-sm border bg-background px-1 text-[11px] text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onChange={(event) => onTileSizeChange(parseGalleryTileSize(event.target.value))}>
              {GALLERY_TILE_SIZES.map((size) => <option key={size.value} value={size.value}>{size.label}</option>)}
            </select>
          </label>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden p-2">
          {tiles.length > 0 ? (
            <VirtualizedMediaGrid
              showFaceBoxes={classification?.showFaceBoxes ?? true}
              customCategories={classification?.custom.categories}
              tileSize={tileSize}
              focusTarget={classification?.focusTarget}
              onClassify={classification?.onClassify}
              classificationSaving={classification?.saving}
              caseDatabasePath={caseDatabasePath}
              mediaType={mediaType}
              onOpenImage={onOpenImage}
              onSelectItem={onSelectItem}
              selectedPath={selectedItem?.path ?? null}
              tiles={tiles}
              totalCount={totalCount ?? tiles.length}
              fetchPage={fetchPage}
            />
          ) : (
            <div className="flex h-full min-h-24 items-center justify-center text-xs text-muted-foreground">
              {isLoading ? "Scanning media..." : emptyText}
            </div>
          )}
        </div>
      </section>

      <MediaDetailsPanel caseDatabasePath={caseDatabasePath} title={detailTitle} item={selectedItem} classification={classification} />
    </div>
  );
}

function VirtualizedMediaGrid({
  showFaceBoxes,
  customCategories,
  tileSize,
  focusTarget,
  onClassify,
  classificationSaving,
  caseDatabasePath,
  mediaType,
  onSelectItem,
  onOpenImage,
  selectedPath,
  tiles,
  totalCount,
  fetchPage,
}: {
  showFaceBoxes: boolean;
  customCategories?: ClassificationDefinition[];
  tileSize: GalleryTileSize;
  focusTarget?: GalleryClassification["focusTarget"];
  onClassify?: GalleryClassification["onClassify"];
  classificationSaving?: boolean;
  caseDatabasePath: string | null;
  mediaType: "image" | "video";
  onSelectItem: (item: MediaItem) => void;
  onOpenImage?: (item: MediaItem) => void;
  selectedPath: string | null;
  tiles: MediaItem[];
  totalCount: number;
  fetchPage: MediaGalleryProps["fetchPage"];
}) {
  const [containerRef, size] = useElementSize<HTMLDivElement>();
  const gridRef = useRef<FixedSizeGrid<VirtualizedMediaGridData>>(null);
  const scrollTopRef = useRef(0);
  const previousLayout = useRef<{ columns: number; rowHeight: number } | null>(null);
  const pendingFocus = useRef<GalleryClassification["focusTarget"]>(null);
  const [revision, setRevision] = useState(0);
  const [visibleRange, setVisibleRange] = useState({ start: 0, stop: 0, direction: 1 });
  const settings = usePerformanceSettings();
  const epoch = useThumbnailEpoch();
  const galleryActive = useGalleryActive();
  const cache = useMemo(() => new VirtualMediaCache(
    { items: tiles, offset: 0, limit: 500, totalCount, hasNextPage: totalCount > tiles.length },
    (offset) => fetchPage(mediaType, offset),
    () => setRevision((value) => value + 1),
  ), [tiles, totalCount, fetchPage, mediaType]);
  useEffect(() => {
    cache.activate();
    return () => cache.dispose();
  }, [cache]);
  const { columnCount, columnWidth } = galleryTileLayout(size.width, tileSize);
  const thumbnailEdge = columnWidth * (window.devicePixelRatio || 1) <= 144 ? 128 : 256;
  const prefetch = useMemo(() => new GalleryPrefetch(
    (index) => cache.itemAt(index),
    (item, priority) => prepareThumbnail(caseDatabasePath!, item, priority, thumbnailEdge),
    thumbnailConcurrency(settings),
  ), [cache, caseDatabasePath, epoch, settings, thumbnailEdge]);
  useEffect(() => () => prefetch.dispose(), [prefetch]);
  useEffect(() => {
    cache.setBudget(recordCacheBytes(settings));
    if (size.height <= 0 || !galleryActive) { prefetch.dispose(); return; }
    const range = galleryPrefetchRange(visibleRange.start, visibleRange.stop, totalCount,
      settings.screensAhead, settings.screensBehind, visibleRange.direction,
      Math.floor(thumbnailCacheBytes(settings) / (thumbnailEdge * thumbnailEdge * 4 + 8192)));
    cache.setRange(range.start, range.stop);
    if (mediaType === "image" && caseDatabasePath) prefetch.update(range);
  }, [cache, prefetch, visibleRange, settings, size.height, galleryActive, totalCount, mediaType, caseDatabasePath, revision, thumbnailEdge]);
  const rowHeight =
    Math.round((columnWidth - MEDIA_TILE_GAP) * MEDIA_TILE_ASPECT_RATIO) +
    (mediaType === "image" ? 38 : 0) +
    MEDIA_TILE_GAP;
  const rowCount = Math.ceil(totalCount / columnCount);
  useLayoutEffect(() => {
    const next = { columns: columnCount, rowHeight };
    const top = galleryScrollAnchor(scrollTopRef.current, previousLayout.current ?? next, next, totalCount, size.height);
    previousLayout.current = next;
    if (top !== scrollTopRef.current) gridRef.current?.scrollTo({ scrollTop: top, scrollLeft: 0 });
  }, [columnCount, rowHeight, totalCount, size.height]);
  useLayoutEffect(() => {
    pendingFocus.current = focusTarget;
    if (focusTarget && galleryActive) {
      gridRef.current?.scrollToItem({ rowIndex: Math.floor(focusTarget.index / columnCount), columnIndex: focusTarget.index % columnCount, align: "smart" });
    }
  }, [focusTarget, columnCount, galleryActive, cache]);
  useLayoutEffect(() => {
    const target = pendingFocus.current;
    if (!target || !galleryActive) return;
    const button = Array.from(containerRef.current?.querySelectorAll<HTMLButtonElement>("button[data-media-path]") ?? [])
      .find((element) => element.dataset.mediaPath === target.path);
    if (button) { button.focus({ preventScroll: true }); pendingFocus.current = null; }
  });
  const gridData: VirtualizedMediaGridData = {
    showFaceBoxes,
    thumbnailEdge,
    customCategories,
    onClassify,
    classificationSaving,
    caseDatabasePath,
    columnCount,
    mediaType,
    onSelectItem,
    onOpenImage,
    selectedPath,
    cache,
  };

  return (
    <div ref={containerRef} className="relative h-full min-h-0 w-full overflow-hidden">
      {cache.hasError && (
        <div role="status" className="absolute right-2 top-2 z-10 flex items-center gap-2 rounded-sm border bg-background p-2 text-xs">
          Some images could not be loaded.
          <Button size="xs" variant="outline" onClick={() => cache.retry()}>Retry</Button>
        </div>
      )}
      {galleryActive && size.width > 0 && size.height > 0 ? (
        <FixedSizeGrid<VirtualizedMediaGridData>
          ref={gridRef}
          className="media-gallery-scroll"
          columnCount={columnCount}
          columnWidth={columnWidth}
          height={size.height}
          itemData={gridData}
          onScroll={({ scrollTop }) => { scrollTopRef.current = scrollTop; }}
          onItemsRendered={({ visibleRowStartIndex, visibleRowStopIndex }) => {
            const start = visibleRowStartIndex * columnCount;
            const stop = Math.min(totalCount - 1, (visibleRowStopIndex + 1) * columnCount - 1);
            setVisibleRange((previous) => previous.start === start && previous.stop === stop ? previous :
              { start, stop, direction: start === previous.start ? previous.direction : start < previous.start ? -1 : 1 });
          }}
          overscanColumnCount={1}
          // Range prefetch handles lookahead without mounting entire screens of
          // menus, observers, and image consumers at small tile sizes.
          overscanRowCount={1}
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
  const tile = data.cache.itemAt(itemIndex);

  if (itemIndex >= data.cache.totalCount) return null;
  if (!tile) {
    return (
      <div style={style} className="p-1" aria-label="Loading media item">
        <div className="h-full w-full animate-pulse rounded-sm border bg-muted" />
      </div>
    );
  }

  return (
    <div style={style} className="p-1">
      <MediaTilePreview
        showFaceBoxes={data.showFaceBoxes}
        thumbnailEdge={data.thumbnailEdge}
        customCategories={data.customCategories}
        itemIndex={itemIndex}
        onClassify={data.onClassify}
        classificationSaving={data.classificationSaving}
        caseDatabasePath={data.caseDatabasePath}
        className="h-full w-full"
        isSelected={data.selectedPath === tile.path}
        mediaType={data.mediaType}
        tile={tile}
        onOpenImage={data.onOpenImage}
        onSelectItem={data.onSelectItem}
      />
    </div>
  );
}

function MediaTilePreview({
  showFaceBoxes,
  thumbnailEdge,
  customCategories,
  itemIndex,
  onClassify,
  classificationSaving,
  caseDatabasePath,
  className,
  isSelected,
  mediaType,
  onSelectItem,
  onOpenImage,
  tile,
}: {
  showFaceBoxes: boolean;
  thumbnailEdge: number;
  customCategories?: ClassificationDefinition[];
  itemIndex: number;
  onClassify?: GalleryClassification["onClassify"];
  classificationSaving?: boolean;
  caseDatabasePath: string | null;
  className?: string;
  isSelected: boolean;
  mediaType: "image" | "video";
  onSelectItem: (item: MediaItem) => void;
  onOpenImage?: (item: MediaItem) => void;
  tile: MediaItem;
}) {
  const classification = mediaType === "image" ? vicClassification(tile.vicCategory, customCategories) : undefined;
  const tileButton = (
    <button
      type="button"
      className={cn(
        "relative overflow-hidden bg-muted text-left",
        mediaType === "image" ? "flex flex-col rounded-lg border-[3px]" : "rounded-sm border-2",
        isSelected && "ring-2 ring-ring ring-offset-2 ring-offset-background",
        className,
      )}
      style={classification ? { borderColor: classification.color } : undefined}
      aria-label={`${tile.name}${mediaType === "image" ? ` · ${classification ? `${classification.value}: ${classification.label}` : "Unclassified"}` : ""}`}
      aria-pressed={isSelected}
      data-media-path={tile.path}
      title={tile.path}
      onDoubleClick={() => onOpenImage?.(tile)}
      onClick={() => onSelectItem(tile)}
      onContextMenu={() => onSelectItem(tile)}
      onKeyDown={(event) => {
        if (event.key === "Enter" && onOpenImage) {
          event.preventDefault(); onOpenImage(tile); return;
        }
        if (onClassify && !classificationSaving && !event.repeat && !event.ctrlKey && !event.altKey && !event.metaKey && /^[0-4]$/.test(event.key)) {
          event.preventDefault(); onSelectItem(tile); onClassify(tile, Number(event.key) as VicCategory, itemIndex);
        }
      }}
    >
      {mediaType === "image" && (tile.stackSize ?? 0) > 1 && <span
        className="pointer-events-none absolute bottom-10 left-1 z-10 inline-flex items-center gap-1 rounded border border-white/20 bg-black/80 px-1.5 py-0.5 text-[10px] font-medium text-white shadow-sm"
        title={`${tile.stackSize} similar images in this stack`} aria-label={`${tile.stackSize} similar images in this stack`}>
        <Layers className="size-3 shrink-0" aria-hidden="true" />
        <span className="tabular-nums">{tile.stackSize}</span>
      </span>}
      {mediaType === "image" ? (
        <MediaImagePreview
          showFaceBoxes={showFaceBoxes}
          caseDatabasePath={caseDatabasePath}
          item={tile}
          retain
          edge={thumbnailEdge}
          fit="contain"
          className="min-h-0 w-full flex-1 bg-black"
        />
      ) : (
        <MediaVideoPreview item={tile} className="h-full w-full" />
      )}
      {mediaType === "image" && <>
        <span className="pointer-events-none block h-[18px] w-full shrink-0 truncate bg-black px-1 text-[10px] leading-[18px] text-white" title={tile.name}>
          {tile.name}
        </span>
        <span className="pointer-events-none block h-5 w-full shrink-0 truncate px-1 text-center text-[11px] font-semibold leading-5"
          style={classification ? { backgroundColor: classification.color, color: classificationTextColor(classification.color) } : undefined}
          title={classification ? `${classification.value} · ${classification.label}` : "Unclassified"}>
          {classification?.label ?? "Unclassified"}
        </span>
      </>}
    </button>
  );

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{tileButton}</ContextMenuTrigger>
      <ContextMenuContent className="min-w-44">
        {onOpenImage && <ContextMenuItem className="text-xs" onSelect={() => onOpenImage(tile)}>
          <Expand className="size-3.5" aria-hidden="true" />Open image
        </ContextMenuItem>}
        {onClassify && <>
          {[...VIC_CATEGORIES, ...(customCategories ?? [])].map((entry) => <ContextMenuItem key={entry.value} className="gap-2 text-xs" disabled={classificationSaving}
            onSelect={() => onClassify(tile, entry.value)}>
            <span className="size-2 rounded-full" style={{ backgroundColor: entry.color }} />{entry.value <= 4 ? `${entry.value} · ` : ""}{entry.label}
          </ContextMenuItem>)}
          <ContextMenuItem className="text-xs" disabled={classificationSaving || tile.vicCategory == null}
            onSelect={() => onClassify(tile, null)}>Clear classification</ContextMenuItem>
          <ContextMenuSeparator />
        </>}
        <ContextMenuItem
          className="text-xs"
          onSelect={() => void revealMediaItemPath(tile)}
        >
          <FolderSearch className="size-3.5" aria-hidden="true" />
          Open file location
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

async function revealMediaItemPath(item: MediaItem) {
  try {
    const path = await invoke<string>("ensure_evidence_file", { path: item.mediaPath || item.path });
    await revealItemInDir(path);
  } catch (error) {
    console.error(`Failed to reveal ${item.path}`, error);
  }
}

function VideoPlayerSheet({
  item,
  onOpenChange,
}: {
  item: MediaItem | null;
  onOpenChange: (item: MediaItem | null) => void;
}) {
  const [prepared, setPrepared] = useState<{ path: string; source: string } | null>(null);
  useEffect(() => {
    let active = true;
    if (item) void invoke<string>("ensure_evidence_file", { path: item.mediaPath }).then((path) => {
      if (active) setPrepared({ path: item.mediaPath, source: convertFileSrc(path) });
    }).catch(console.error);
    return () => { active = false; };
  }, [item?.mediaPath]);
  const source = item && prepared?.path === item.mediaPath ? prepared.source : "";

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
  caseDatabasePath,
  classification,
  item,
  title,
}: {
  caseDatabasePath: string | null;
  classification?: GalleryClassification;
  item: MediaItem | null;
  title: string;
}) {
  const galleryActive = useGalleryActive();
  const selectedClassification = item?.mediaType === "image" ? vicClassification(item.vicCategory, classification?.custom.categories) : undefined;
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
        {item && galleryActive && <section className="mb-2 overflow-hidden rounded-lg border-2" aria-label="Selected media preview"
          style={selectedClassification ? { borderColor: selectedClassification.color } : undefined}>
          {item.mediaType === "image" ? <MediaImagePreview caseDatabasePath={caseDatabasePath} item={item} fit="contain" retain showFaceBoxes={classification?.showFaceBoxes ?? true}
            className="aspect-[4/3] w-full bg-black" /> : <MediaVideoPreview item={item} className="aspect-[4/3] w-full bg-black" />}
          <div className="truncate bg-black px-1 py-0.5 text-[11px] text-white" title={item.name}>{item.name}</div>
          {selectedClassification && <div className="break-words px-1 py-0.5 text-center text-[11px] font-semibold"
            style={{ backgroundColor: selectedClassification.color, color: classificationTextColor(selectedClassification.color) }}>{selectedClassification.label}</div>}
        </section>}
        {item?.visualStackId && (item.stackSize ?? 0) > 1 && classification?.visual && classification.visual.stackId !== item.visualStackId && (
          <Button size="sm" variant="outline" onClick={() => classification.visual?.onOpenStack(item.visualStackId!)}>Open similar stack ({item.stackSize})</Button>
        )}
        {classification && <VicClassificationControls category={item?.vicCategory} customCategories={classification.custom.categories}
          disabled={!item || classification.saving}
          onClassify={(category) => { if (item) classification.onClassify(item, category); }} />}
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
