import { useEffect, useRef, useState } from "react";
import { loadArtifactRowTags, setArtifactRowTag, type ArtifactRowTag } from "../services/artifactTagRepository";

const message = (error: unknown) => error instanceof Error ? error.message : String(error);

export function useArtifactRowTags(databasePath: string, viewKey: string, rowKeys: string[], enabled: boolean) {
  const [tags, setTags] = useState<Record<string, ArtifactRowTag[]>>({});
  const [availableTags, setAvailableTags] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const version = useRef(0);
  const writing = useRef(false);
  const keys = JSON.stringify(rowKeys);

  useEffect(() => {
    const current = ++version.current;
    setTags({}); setError(""); setReady(false);
    if (enabled && viewKey) {
      void loadArtifactRowTags(databasePath, viewKey, JSON.parse(keys) as string[])
        .then((result) => {
          if (version.current !== current) return;
          setTags(result.rows); setAvailableTags(result.availableTags); setReady(true);
        }).catch((error: unknown) => { if (version.current === current) setError(message(error)); });
    }
    return () => { version.current++; };
  }, [databasePath, viewKey, keys, enabled, retry]);

  async function assign(rowKey: string, tagName: string, assigned: boolean) {
    if (writing.current) throw new Error("Wait for the current tag change to finish.");
    const current = version.current;
    writing.current = true; setBusy(true); setError("");
    try {
      const result = await setArtifactRowTag(databasePath, viewKey, rowKey, tagName, assigned);
      if (version.current === current) {
        setTags((previous) => ({ ...previous, [rowKey]: result.rows[rowKey] ?? [] }));
        setAvailableTags(result.availableTags);
      }
    } catch (error) {
      if (version.current === current) setError(message(error));
      throw error;
    } finally { writing.current = false; setBusy(false); }
  }

  return { tags, availableTags, busy, ready, error, assign, refresh: () => setRetry((value) => value + 1) };
}
