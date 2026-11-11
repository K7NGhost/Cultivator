import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import type { GeoJSONSource, Map as LibreMap } from "maplibre-gl";
import workerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";
import "maplibre-gl/dist/maplibre-gl.css";
import { basemapStyle, localStyle, type Basemap } from "@/lib/maps/mapStyles";
import { hasBasemapStyle, installBasemapImageResolver } from "@/lib/maps/mapImages";
import { featureAtSourceIndex, geometryBounds, renderCollection, renderGeometry } from "../lib/mapFeatures";
import type { FeatureSelection, VisibleMapLayer } from "../types";

export type MapHandle = { fitAll: () => void; fitLayer: (id: string) => void; fitSelection: () => void; retry: () => void };
type Props = {
  basemap: Basemap;
  layers: VisibleMapLayer[];
  selected: FeatureSelection | null;
  onSelect: (selection: FeatureSelection) => void;
  onStatus: (message: string) => void;
};
const SOURCE = "map-file-features", SELECTED = "map-file-selected";
const HIT_LAYERS = ["map-file-points", "map-file-lines", "map-file-polygons"];

export const MapCanvas = forwardRef<MapHandle, Props>(function MapCanvas(props, ref) {
  const container = useRef<HTMLDivElement>(null);
  const mapRef = useRef<LibreMap | null>(null);
  const latest = useRef(props);
  latest.current = props;
  const updateRef = useRef<(selectionOnly?: boolean) => void>(() => {});
  const fitRef = useRef<(id?: string, selection?: boolean) => void>(() => {});
  const reloadRef = useRef(() => {});
  useImperativeHandle(ref, () => ({ fitAll: () => fitRef.current(), fitLayer: (id) => fitRef.current(id),
    fitSelection: () => fitRef.current(undefined, true), retry: () => reloadRef.current() }), []);

  useEffect(() => {
    const element = container.current;
    if (!element) return;
    let disposed = false, local = false, fitted = false;
    let map: LibreMap | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let resize: ResizeObserver | undefined, theme: MutationObserver | undefined;
    latest.current.onStatus("Loading map…");
    void import("maplibre-gl").then((library) => {
      if (disposed) return;
      library.setWorkerUrl(workerUrl);
      map = new library.Map({ container: element, style: basemapStyle(latest.current.basemap), center: [0, 0], zoom: 1,
        attributionControl: { compact: true } });
      mapRef.current = map;
      installBasemapImageResolver(map);
      map.getCanvas().setAttribute("aria-label", "Interactive map. Select a point, track, or polygon to inspect its properties.");
      map.addControl(new library.NavigationControl(), "top-right");
      map.addControl(new library.ScaleControl({ maxWidth: 100, unit: "metric" }), "bottom-left");
      fitRef.current = (layerId, selection) => {
        if (!map) return;
        const { layers, selected } = latest.current;
        const selectedLayer = layers.find((layer) => layer.id === selected?.layerId);
        const selectedFeature = selected && selectedLayer ? featureAtSourceIndex(selectedLayer, selected.index) : undefined;
        const features = selection ? selectedFeature ? [selectedFeature] : []
          : layers.filter((layer) => !layerId || layer.id === layerId).flatMap((layer) => layer.collection.features);
        const bounds = geometryBounds(features.map((item) => item.geometry));
        if (bounds) map.fitBounds(bounds, { padding: 55, maxZoom: 17, duration: 350 });
      };
      updateRef.current = (selectionOnly = false) => {
        if (!map?.getSource(SOURCE)) return;
        if (!selectionOnly) (map.getSource(SOURCE) as GeoJSONSource).setData(renderCollection(latest.current.layers));
        const { selected, layers } = latest.current;
        const layer = layers.find((layer) => layer.id === selected?.layerId);
        const feature = layer && selected ? featureAtSourceIndex(layer, selected.index) : undefined;
        (map.getSource(SELECTED) as GeoJSONSource).setData({ type: "FeatureCollection", features: feature ? [{ ...feature, geometry: renderGeometry(feature.geometry) }] : [] });
        if (!fitted && layers.some((layer) => layer.collection.features.length > 0)) { fitRef.current(); fitted = true; }
      };
      const color = ["get", "_cvColor"] as const;
      map.on("style.load", () => {
        if (!map || disposed) return;
        clearTimeout(timeout);
        map.addSource(SOURCE, { type: "geojson", data: renderCollection(latest.current.layers) });
        map.addSource(SELECTED, { type: "geojson", data: { type: "FeatureCollection", features: [] } });
        map.addLayer({ id: "map-file-polygons", type: "fill", source: SOURCE, filter: ["==", ["geometry-type"], "Polygon"],
          paint: { "fill-color": [...color], "fill-opacity": 0.2 } });
        map.addLayer({ id: "map-file-lines", type: "line", source: SOURCE,
          filter: ["in", ["geometry-type"], ["literal", ["LineString", "Polygon"]]],
          layout: { "line-cap": "round", "line-join": "round" }, paint: { "line-color": [...color], "line-width": 3 } });
        map.addLayer({ id: "map-file-points", type: "circle", source: SOURCE, filter: ["==", ["geometry-type"], "Point"],
          paint: { "circle-color": [...color], "circle-radius": 5, "circle-stroke-color": "#ffffff", "circle-stroke-width": 1.5 } });
        map.addLayer({ id: "map-selected-outline", type: "line", source: SELECTED,
          filter: ["in", ["geometry-type"], ["literal", ["LineString", "Polygon"]]],
          paint: { "line-color": "#facc15", "line-width": 5, "line-opacity": 0.85 } });
        map.addLayer({ id: "map-selected-point", type: "circle", source: SELECTED, filter: ["==", ["geometry-type"], "Point"],
          paint: { "circle-color": "#facc15", "circle-radius": 8, "circle-stroke-color": "#111827", "circle-stroke-width": 2 } });
        updateRef.current();
        latest.current.onStatus(local ? "Basemap unavailable. Map features remain visible." : "");
      });
      map.on("click", (event) => {
        if (!map || !HIT_LAYERS.every((layer) => map?.getLayer(layer))) return;
        const feature = map.queryRenderedFeatures(event.point, { layers: HIT_LAYERS })[0];
        const layerId: unknown = feature?.properties._cvLayer;
        const index = Number(feature?.properties._cvIndex);
        if (typeof layerId === "string" && Number.isInteger(index)) latest.current.onSelect({ layerId, index });
      });
      map.on("mousemove", (event) => {
        if (!map || !HIT_LAYERS.every((layer) => map?.getLayer(layer))) return;
        map.getCanvas().style.cursor = map.queryRenderedFeatures(event.point, { layers: HIT_LAYERS }).length ? "pointer" : "";
      });
      function fallback() {
        if (!map || disposed || local) return;
        local = true; clearTimeout(timeout);
        latest.current.onStatus("Basemap unavailable. Map features remain visible.");
        map.setStyle(localStyle());
      }
      reloadRef.current = () => {
        if (!map || disposed) return;
        local = false; clearTimeout(timeout);
        latest.current.onStatus("Loading basemap…");
        timeout = setTimeout(fallback, 30_000);
        map.setStyle(basemapStyle(latest.current.basemap));
      };
      map.on("error", () => {
        if (disposed || local || !map) return;
        if (!hasBasemapStyle(map)) fallback();
        else latest.current.onStatus("Some basemap resources could not load. Map features remain available.");
      });
      timeout = setTimeout(fallback, 30_000);
      resize = new ResizeObserver(() => map?.resize()); resize.observe(element);
      theme = new MutationObserver(() => {
        if (local) map?.setStyle(localStyle()); else reloadRef.current();
      });
      theme.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    }).catch((caught: unknown) => {
      if (!disposed) latest.current.onStatus(`Cannot initialize map. WebGL is required. ${String(caught)}`);
    });
    return () => {
      disposed = true; clearTimeout(timeout); resize?.disconnect(); theme?.disconnect();
      updateRef.current = () => {}; fitRef.current = () => {}; reloadRef.current = () => {};
      map?.remove(); mapRef.current = null;
    };
  }, []);
  useEffect(() => { updateRef.current(); }, [props.layers]);
  useEffect(() => { updateRef.current(true); }, [props.selected]);
  useEffect(() => { if (mapRef.current) reloadRef.current(); }, [props.basemap]);
  return <div ref={container} className="absolute! inset-0 [&_.maplibregl-ctrl-attrib]:bg-card! [&_.maplibregl-ctrl-attrib]:text-foreground! [&_.maplibregl-ctrl-scale]:bg-card! [&_.maplibregl-ctrl-scale]:text-foreground!" aria-label="Map files canvas" />;
});
