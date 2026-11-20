import { useEffect, useRef, useState } from "react";
import { loadTaggedArtifactCatalog, subscribeArtifactTagChanges } from "../services/artifactTagRepository";

const emptyCounts: ReadonlyMap<string, number> = new Map();

export function useTaggedArtifactCatalog(databasePath: string, revision: string, active: boolean) {
  const [snapshot, setSnapshot] = useState<{ databasePath: string; revision: string; counts: ReadonlyMap<string, number> } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const version = useRef(0);
  useEffect(() => subscribeArtifactTagChanges(databasePath, () => setRetry((value) => value + 1)), [databasePath]);
  useEffect(() => {
    const current = ++version.current;
    setError("");
    if (!databasePath || !active) { setLoading(false); return; }
    setLoading(true);
    void loadTaggedArtifactCatalog(databasePath).then((rows) => {
      if (version.current !== current) return;
      setSnapshot({ databasePath, revision, counts: new Map(rows.map((row) => [row.artifactId, row.entryCount])) });
    }).catch((error: unknown) => {
      if (version.current !== current) return;
      setSnapshot(null); setError(error instanceof Error ? error.message : String(error));
    }).finally(() => { if (version.current === current) setLoading(false); });
    return () => { version.current++; };
  }, [databasePath, revision, active, retry]);
  const ready = snapshot?.databasePath === databasePath && snapshot.revision === revision;
  return { counts: ready ? snapshot.counts : emptyCounts, ready, loading, error, refresh: () => setRetry((value) => value + 1) };
}
