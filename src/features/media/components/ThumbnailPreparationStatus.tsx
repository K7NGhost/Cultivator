import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { getThumbnailPreparationStatus, prepareCaseThumbnails, type ThumbnailPreparationStatus as Status } from "../services/mediaRepository";
import { clearPreparedThumbnails } from "../services/preparedThumbnails";

export function ThumbnailPreparationStatus({ caseDatabasePath }: { caseDatabasePath: string | null }) {
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setStatus(null); setError(null);
    if (!caseDatabasePath) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await getThumbnailPreparationStatus(caseDatabasePath);
        if (!disposed) { setStatus(next); setError(next.error); }
      } catch (error) { if (!disposed) setError(String(error)); }
      finally { if (!disposed) timer = setTimeout(poll, 1500); }
    };
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [caseDatabasePath]);
  if (!caseDatabasePath || (!error && (!status || status.total === 0 && !status.scanning))) return null;
  const retry = () => {
    void prepareCaseThumbnails(caseDatabasePath, { retryFailed: true, rescan: true })
      .then(next => { setStatus(next); setError(next.error); clearPreparedThumbnails(); }, error => setError(String(error)));
  };
  return <span className="inline-flex min-w-0 max-w-48 items-center gap-1 text-[11px] text-muted-foreground" role="status" title={error ?? undefined}>
    {error ? "Preview preparation needs attention" : status?.scanning ? "Finding previews…" : status?.pending ?
      `Preparing previews ${status.ready.toLocaleString()}/${status.total.toLocaleString()}` :
      status?.checking ? `Checking previews ${(status.total - status.checking).toLocaleString()}/${status.total.toLocaleString()}` :
      status?.failed ? `${status.failed.toLocaleString()} previews unavailable` : "Previews ready"}
    {(error || !!status?.failed) && <Button variant="ghost" size="xs" className="h-5 px-1" onClick={retry}>Retry</Button>}
  </span>;
}
