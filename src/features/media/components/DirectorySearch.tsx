import { useEffect, useMemo, useState } from "react";
import { FixedSizeList, type ListChildComponentProps } from "react-window";
import { FileImage, Film, FolderSearch, Folder, Search, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { listMediaDirectories, listMediaGalleryPage, listClassifiedFileViewMediaPage, type MediaDirectory, type FileViewMediaPage } from "../services/mediaRepository";

type Props = {
  caseDatabasePath: string;
  datasourceId: string;
  roots: string[];
  indexed: boolean;
  mediaType: "image" | "video";
  selectedDirectory: string | null;
  onSelect(directory: string | null): void;
  revision: number;
};

export function DirectorySearch({ caseDatabasePath, datasourceId, roots, indexed, mediaType, selectedDirectory, onSelect, revision }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [directories, setDirectories] = useState<MediaDirectory[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const [retry, setRetry] = useState(0);
  const [browsing, setBrowsing] = useState<string | null>(null);
  const [type, setType] = useState(mediaType);
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<FileViewMediaPage>();
  const [filesLoading, setFilesLoading] = useState(false);
  const [filesError, setFilesError] = useState<string>();

  useEffect(() => {
    if (!open) return;
    let current = true;
    setLoading(true); setError(undefined); setDirectories([]);
    listMediaDirectories(caseDatabasePath, datasourceId, indexed).then(result => {
      if (current) setDirectories(result);
    }, reason => { if (current) setError(String(reason)); }).finally(() => { if (current) setLoading(false); });
    return () => { current = false; };
  }, [open, caseDatabasePath, datasourceId, indexed, revision, retry]);

  useEffect(() => {
    if (!open || !browsing) return;
    let current = true;
    setFilesLoading(true); setFilesError(undefined); setPage(undefined);
    const request = indexed
      ? listMediaGalleryPage(caseDatabasePath, datasourceId, type, offset, [], browsing)
      : listClassifiedFileViewMediaPage(caseDatabasePath, roots, type, offset, [], browsing);
    request.then(result => { if (current) setPage(result); }, reason => { if (current) setFilesError(String(reason)); })
      .finally(() => { if (current) setFilesLoading(false); });
    return () => { current = false; };
  }, [open, browsing, type, offset, caseDatabasePath, datasourceId, indexed, roots, revision, retry]);

  const matches = useMemo(() => directories.filter(directory => directory.path.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())), [directories, query]);
  const data = { directories: matches, browsing, select: (path: string) => { setBrowsing(path); setOffset(0); } };
  return <div className="grid gap-2 border-b p-2 text-xs">
    <Dialog open={open} onOpenChange={value => {
      setOpen(value);
      if (value) { setBrowsing(selectedDirectory); setType(mediaType); setOffset(0); setQuery(""); }
    }}>
      <DialogTrigger asChild>
        <Button type="button" variant="outline" size="sm" className="w-full justify-start text-xs">
          <FolderSearch className="size-3.5" />Directory search
        </Button>
      </DialogTrigger>
      <DialogContent className="flex max-h-[90vh] flex-col sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Search media directories</DialogTitle>
          <DialogDescription>Only directories containing images or videos are shown. Select a directory to see its files, or filter the gallery to files directly inside it.</DialogDescription>
        </DialogHeader>
        <div className="relative">
          <Search className="absolute left-3 top-2.5 size-4 text-muted-foreground" aria-hidden="true" />
          <Input aria-label="Search directory paths" placeholder="Search directory paths…" className="pl-9" value={query} onChange={event => setQuery(event.target.value)} />
        </div>
        <div className="grid min-h-0 gap-3 overflow-auto sm:grid-cols-2">
          <section className="min-w-0 rounded-md border" aria-label="Media directories" aria-busy={loading}>
            <div className="border-b px-3 py-2 text-xs text-muted-foreground">{loading ? "Finding media directories…" : `${matches.length.toLocaleString()} directories`}</div>
            {error ? <div className="grid gap-2 p-3 text-xs"><p role="alert" className="break-words text-destructive">{error}</p><Button size="sm" variant="outline" onClick={() => setRetry(value => value + 1)}>Retry</Button></div>
              : loading ? <p role="status" className="p-3 text-xs text-muted-foreground">Looking through the selected datasource…</p>
              : !matches.length ? <p className="p-3 text-xs text-muted-foreground">{query ? "No directories match your search." : "No directories containing media were found."}</p>
              : <FixedSizeList key={query} height={320} width="100%" itemCount={matches.length} itemSize={68} itemData={data} itemKey={(index, value) => value.directories[index].path}>
                {DirectoryRow}
              </FixedSizeList>}
          </section>
          <section className="flex min-h-0 min-w-0 flex-col rounded-md border" aria-label="Files in selected directory" aria-busy={filesLoading}>
            <div className="border-b p-2">
              <p className="truncate text-xs font-medium" title={browsing ?? undefined}>{browsing ?? "Choose a directory"}</p>
              <div className="mt-2 flex gap-1">
                {(["image", "video"] as const).map(value => <Button key={value} size="xs" variant={type === value ? "secondary" : "ghost"}
                  aria-pressed={type === value} onClick={() => { setType(value); setOffset(0); }}>{value === "image" ? "Images" : "Videos"}</Button>)}
              </div>
            </div>
            <div className="h-64 overflow-auto p-2 text-xs">
              {filesLoading ? <p role="status" className="p-1 text-muted-foreground">Loading files…</p>
                : filesError ? <div className="grid gap-2"><p role="alert" className="break-words text-destructive">{filesError}</p><Button size="xs" variant="outline" onClick={() => setRetry(value => value + 1)}>Retry</Button></div>
                : !browsing ? <p className="p-1 text-muted-foreground">Select a directory to view its media files.</p>
                : !page?.items.length ? <p className="p-1 text-muted-foreground">No {type === "image" ? "images" : "videos"} directly in this directory.</p>
                : <ul>{page.items.map(item => <li key={item.path} className="flex items-start gap-2 border-b px-1 py-2 last:border-0">
                  {type === "image" ? <FileImage className="size-3.5 shrink-0" /> : <Film className="size-3.5 shrink-0" />}
                  <span className="min-w-0 break-all" title={item.path}>{item.name}</span>
                </li>)}</ul>}
            </div>
            {page && <div className="mt-auto flex items-center justify-between gap-2 border-t p-2 text-[11px]">
              <span>{page.totalCount ? `${offset + 1}–${offset + page.items.length} of ${page.totalCount.toLocaleString()}` : "0 files"}</span>
              <div className="flex gap-1"><Button size="xs" variant="outline" disabled={offset === 0 || filesLoading} onClick={() => setOffset(value => Math.max(0, value - 500))}>Previous</Button>
                <Button size="xs" variant="outline" disabled={!page.hasNextPage || filesLoading} onClick={() => setOffset(value => value + 500)}>Next</Button></div>
            </div>}
          </section>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>Close</Button>
          <Button disabled={!browsing || loading || !!error || !directories.some(directory => directory.path === browsing)} onClick={() => { onSelect(browsing); setOpen(false); }}>Filter to this directory</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
    {selectedDirectory && <div className="flex min-w-0 items-start gap-1 rounded-sm bg-accent p-1.5">
      <span className="min-w-0 flex-1 break-all text-[11px]" title={selectedDirectory}>{selectedDirectory}</span>
      <button type="button" className="shrink-0 rounded-sm p-0.5 hover:bg-muted" aria-label="Clear directory filter" onClick={() => onSelect(null)}><X className="size-3.5" /></button>
    </div>}
  </div>;
}

function DirectoryRow({ index, style, data }: ListChildComponentProps<{ directories: MediaDirectory[]; browsing: string | null; select(path: string): void }>) {
  const directory = data.directories[index];
  return <button type="button" style={style} className="flex w-full items-center gap-2 border-b px-3 text-left text-xs hover:bg-accent aria-pressed:bg-accent"
    aria-pressed={data.browsing === directory.path} title={directory.path} onClick={() => data.select(directory.path)}>
    <Folder className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
    <span className="min-w-0"><span className="block truncate font-medium">{directory.path.split("/").filter(Boolean).pop() ?? directory.path}</span>
      <span className="block truncate text-[11px] text-muted-foreground">{directory.path}</span>
      <span className="block text-[11px] text-muted-foreground">{directory.imageCount.toLocaleString()} images · {directory.videoCount.toLocaleString()} videos</span>
    </span>
  </button>;
}
