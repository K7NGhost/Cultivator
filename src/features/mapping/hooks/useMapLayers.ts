import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { listMapLayers, loadMapLayer } from "../services/mapRepository";
import type { MapLayerData, MapLayerSummary } from "../types";

export function useMapLayers(databasePath: string, active: boolean) {
  const [layers, setLayers] = useState<MapLayerSummary[]>([]);
  const [visible, setVisible] = useState<string[]>([]);
  const [data, setData] = useState<Record<string, MapLayerData>>({});
  const [refreshing, setRefreshing] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const initialized = useRef(false);
  const previousLayers = useRef<MapLayerSummary[]>([]);
  const mounted = useRef(true);
  const request = useRef(0);

  const refresh = useCallback(async (quiet = false) => {
    const version = ++request.current;
    if (!quiet) { setRefreshing(true); setError(null); }
    try {
      const next = await listMapLayers(databasePath);
      if (!mounted.current || version !== request.current) return;
      const previousLayersForIds = previousLayers.current;
      previousLayers.current = next;
      // React Strict Mode may call state updaters twice. Capture this decision
      // once so both calls return the same initial visibility.
      const selectInitialLayer = !initialized.current && next.length > 0;
      if (selectInitialLayer) initialized.current = true;
      setLayers((previous) => JSON.stringify(previous) === JSON.stringify(next) ? previous : next);
      setVisible((previous) => {
        if (selectInitialLayer) return [next[0].id];
        const kept = previous.flatMap((id) => {
          if (next.some((layer) => layer.id === id)) return [id];
          const old = previousLayersForIds.find((layer) => layer.id === id);
          const replacement = next.find((layer) => layer.filePath === old?.filePath && layer.datasourceId === old?.datasourceId);
          return replacement ? [replacement.id] : [];
        });
        return kept.length === previous.length && kept.every((id, index) => previous[index] === id) ? previous : kept;
      });
      setData((previous) => Object.keys(previous).every((id) => next.some((layer) => layer.id === id)) ? previous
        : Object.fromEntries(Object.entries(previous).filter(([id]) => next.some((layer) => layer.id === id))));
    } catch (caught) {
      if (mounted.current && version === request.current) setError(String(caught));
    } finally {
      if (mounted.current && version === request.current) setRefreshing(false);
    }
  }, [databasePath]);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const subscription = listen("plugin-run-finished", () => void refresh());
    const timer = window.setInterval(() => { if (active) void refresh(true); }, 5000);
    return () => { mounted.current = false; window.clearInterval(timer); void subscription.then((stop) => stop()).catch(() => {}); };
  }, [refresh, active]);

  useEffect(() => {
    const missing = visible.filter((id) => !data[id]);
    if (!missing.length) { setLoading(false); return; }
    let disposed = false;
    setLoading(true);
    void Promise.allSettled(missing.map(async (id) => [id, await loadMapLayer(databasePath, id)] as const)).then((results) => {
      if (disposed) return;
      const loaded: Record<string, MapLayerData> = {};
      for (const result of results) {
        if (result.status === "fulfilled") loaded[result.value[0]] = result.value[1];
        else setError(String(result.reason));
      }
      // Failed IDs are removed from visibility so a bad payload is not retried
      // indefinitely. The user can toggle the layer to retry after fixing it.
      setVisible((previous) => previous.filter((id) => !missing.includes(id) || !!loaded[id]));
      setData((previous) => ({ ...previous, ...loaded }));
      setLoading(false);
    });
    return () => { disposed = true; };
  }, [databasePath, visible, data]);

  return { layers, visible, setVisible, data, refreshing, loading, error, refresh };
}
