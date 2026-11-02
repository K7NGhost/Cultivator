import { basemapStyle, localStyle, type Basemap } from "@/lib/maps/mapStyles";
import { hasBasemapStyle, installBasemapImageResolver } from "@/lib/maps/mapImages";
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Flag, MapPin } from "lucide-react";
import quadcopterIcon from "../assets/quadcopter.svg";
import type {
  Marker,
  Popup,
  GeoJSONSource,
  Map as MapLibreMap,
} from "maplibre-gl";
import workerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";
import "maplibre-gl/dist/maplibre-gl.css";
import {
  flightGeoJson,
  mapCoordinates,
  selectedGeoJson,
  type GpsRecord,
} from "../lib/mapData";
import type { FlightRecord } from "../types";

const FLIGHT_PATH_COLOR = "#f97316";

export type MapHandle = {
  zoomIn: () => void;
  zoomOut: () => void;
  fitFlight: () => void;
  resetNorth: () => void;
  retry: () => void;
};
export type MapStatus = {
  phase: "loading" | "ready" | "failed";
  offline?: boolean;
  message?: string;
};
export type { Basemap } from "@/lib/maps/mapStyles";

type Props = {
  basemap: Basemap;
  records: GpsRecord[];
  selected: FlightRecord;
  showPath: boolean;
  onStatus: (status: MapStatus) => void;
};

function updateDroneMarker(
  map: MapLibreMap,
  marker: Marker | null,
  selected: ReturnType<typeof selectedGeoJson>,
) {
  if (!marker) return;
  const coordinate = selected.features[0]?.geometry.coordinates;
  if (!coordinate) {
    marker.remove();
    return;
  }
  marker.setLngLat([coordinate[0], coordinate[1]]).addTo(map);
}

function updateEndpoints(
  map: MapLibreMap,
  popups: Popup[],
  records: GpsRecord[],
  visible: boolean,
) {
  const coordinates = mapCoordinates(records);
  const endpoints = [coordinates[0], coordinates[coordinates.length - 1]];
  popups.forEach((popup, index) => {
    const coordinate = endpoints[index];
    if (!visible || !coordinate) popup.remove();
    else {
      popup.setLngLat(coordinate);
      if (!popup.isOpen()) popup.addTo(map);
    }
  });
}

export const MapCanvas = forwardRef<MapHandle, Props>(
  function MapCanvas(props, ref) {
    const container = useRef<HTMLDivElement>(null);
    const mapRef = useRef<MapLibreMap | null>(null);
    const markerRef = useRef<Marker | null>(null);
    const endpointRefs = useRef<Popup[]>([]);
    const latest = useRef(props);
    latest.current = props;
    const fitRef = useRef<() => void>(() => {});
    const retryRef = useRef<() => void>(() => {});

    useImperativeHandle(
      ref,
      () => ({
        zoomIn: () => {
          mapRef.current?.zoomIn();
        },
        zoomOut: () => {
          mapRef.current?.zoomOut();
        },
        fitFlight: () => fitRef.current(),
        resetNorth: () => {
          mapRef.current?.resetNorth();
        },
        retry: () => retryRef.current(),
      }),
      [],
    );

    useEffect(() => {
      if (!container.current) return;
      const element = container.current;
      let disposed = false;
      let resizeObserver: ResizeObserver | undefined;
      let themeObserver: MutationObserver | undefined;
      let loadTimeout: number | undefined;
      let map: MapLibreMap | undefined;
      let markerRoot: Root | undefined;
      const endpointRoots: Root[] = [];
      let local = false;
      let fitted = false;

      function updateData() {
        if (!map) return;
        const { records, selected, showPath } = latest.current;
        const data = flightGeoJson(records);
        for (const [id, geojson] of Object.entries(data))
          (map.getSource(`flight-${id}`) as GeoJSONSource | undefined)?.setData(
            geojson,
          );
        const selectedData = selectedGeoJson(
          selected,
          mapCoordinates(records)[0]?.[0],
        );
        (
          map.getSource("flight-selected") as GeoJSONSource | undefined
        )?.setData(selectedData);
        updateDroneMarker(map, markerRef.current, selectedData);
        updateEndpoints(map, endpointRefs.current, records, showPath);
        for (const id of ["flight-route"])
          if (map.getLayer(id))
            map.setLayoutProperty(
              id,
              "visibility",
              showPath ? "visible" : "none",
            );
        map
          .getCanvas()
          .setAttribute(
            "aria-label",
            `Interactive flight map. Selected record ${selected.id} at ${selected.time}.`,
          );
      }

      function addFlightLayers() {
        if (!map) return;
        const data = flightGeoJson(latest.current.records);
        for (const [id, geojson] of Object.entries(data))
          if (!map.getSource(`flight-${id}`))
            map.addSource(`flight-${id}`, { type: "geojson", data: geojson });
        if (!map.getSource("flight-selected"))
          map.addSource("flight-selected", {
            type: "geojson",
            data: selectedGeoJson(latest.current.selected),
          });
        if (!map.getLayer("flight-route"))
          map.addLayer({
            id: "flight-route",
            type: "line",
            source: "flight-route",
            layout: { "line-cap": "round", "line-join": "round" },
            paint: { "line-color": FLIGHT_PATH_COLOR, "line-width": 3.5 },
          });
        updateData();
      }

      void import("maplibre-gl")
        .then((library) => {
          if (disposed) return;
          library.setWorkerUrl(workerUrl);
          latest.current.onStatus({ phase: "loading" });
          map = new library.Map({
            container: element,
            style: basemapStyle(latest.current.basemap),
            center: [0, 0],
            zoom: 1,
            attributionControl: { compact: true },
          });
          mapRef.current = map;
          installBasemapImageResolver(map);
          const droneElement = document.createElement("div");
          droneElement.className =
            "flex size-7 items-center justify-center text-primary";
          droneElement.setAttribute("role", "img");
          droneElement.setAttribute("aria-label", "Drone position");
          markerRoot = createRoot(droneElement);
          markerRoot.render(
            <span
              className="block size-7 bg-current"
              style={{
                mask: `url("${quadcopterIcon}") center / contain no-repeat`,
                WebkitMask: `url("${quadcopterIcon}") center / contain no-repeat`,
              }}
              aria-hidden="true"
            />,
          );
          markerRef.current = new library.Marker({
            element: droneElement,
            anchor: "center",
          });
          for (const [index, label] of ["Start", "End"].entries()) {
            const endpoint = document.createElement("div");
            endpoint.className =
              "flex items-center gap-1.5 text-xs font-semibold";
            endpoint.setAttribute("role", "img");
            endpoint.setAttribute(
              "aria-label",
              index === 0
                ? "Flight start: first GPS fix"
                : "Flight end: last GPS fix",
            );
            const root = createRoot(endpoint);
            root.render(
              <>
                {index === 0 ? (
                  <Flag size={14} aria-hidden="true" />
                ) : (
                  <MapPin size={14} aria-hidden="true" />
                )}
                {label}
              </>,
            );
            endpointRoots.push(root);
            endpointRefs.current.push(
              new library.Popup({
                closeButton: false,
                closeOnClick: false,
                closeOnMove: false,
                focusAfterOpen: false,
                maxWidth: "140px",
                className:
                  index === 0
                    ? "flight-endpoint-popup flight-start-popup"
                    : "flight-endpoint-popup flight-end-popup",
              }).setDOMContent(endpoint),
            );
          }
          map.addControl(
            new library.ScaleControl({ maxWidth: 100, unit: "metric" }),
            "bottom-left",
          );
          fitRef.current = () => {
            if (!map || !latest.current.records.length) return;
            const coordinates = mapCoordinates(latest.current.records);
            const bounds = new library.LngLatBounds(
              coordinates[0],
              coordinates[0],
            );
            coordinates.forEach((point) => bounds.extend(point));
            map.fitBounds(bounds, {
              padding: { top: 80, bottom: 50, left: 45, right: 65 },
              maxZoom: 18,
              duration: 400,
            });
          };
          function fallback() {
            if (!map || disposed || local) return;
            local = true;
            window.clearTimeout(loadTimeout);
            latest.current.onStatus({
              phase: "ready",
              offline: true,
              message: "Basemap unavailable. The GPS route remains visible.",
            });
            map.setStyle(localStyle());
          }
          function loadBasemap() {
            if (!map || disposed) return;
            local = false;
            latest.current.onStatus({ phase: "loading" });
            window.clearTimeout(loadTimeout);
            loadTimeout = window.setTimeout(fallback, 30_000);
            map.setStyle(basemapStyle(latest.current.basemap));
          }
          retryRef.current = loadBasemap;
          map.on("style.load", () => {
            if (disposed) return;
            window.clearTimeout(loadTimeout);
            addFlightLayers();
            if (!fitted && latest.current.records.length) {
              fitted = true;
              fitRef.current();
            }
            latest.current.onStatus({
              phase: "ready",
              offline: local,
              message: local
                ? "Basemap unavailable. The GPS route remains visible."
                : undefined,
            });
          });
          map.on("error", () => {
            if (disposed || local) return;
            if (!map || !hasBasemapStyle(map)) fallback();
            else
              latest.current.onStatus({
                phase: "ready",
                offline: true,
                message:
                  "Some basemap resources could not load. The GPS route remains available.",
              });
          });
          resizeObserver = new ResizeObserver(() => map?.resize());
          resizeObserver.observe(element);
          themeObserver = new MutationObserver(() => {
            if (
              latest.current.basemap === "satellite" &&
              !local &&
              map?.getLayer("flight-route")
            )
              map.setPaintProperty(
                "flight-route",
                "line-color",
                FLIGHT_PATH_COLOR,
              );
            else loadBasemap();
          });
          themeObserver.observe(document.documentElement, {
            attributes: true,
            attributeFilter: ["class"],
          });
          loadTimeout = window.setTimeout(fallback, 30_000);
        })
        .catch((error) => {
          if (!disposed)
            latest.current.onStatus({
              phase: "failed",
              message: `Cannot initialize the map. WebGL is required. ${String(error)}`,
            });
        });

      return () => {
        disposed = true;
        window.clearTimeout(loadTimeout);
        resizeObserver?.disconnect();
        themeObserver?.disconnect();
        endpointRoots.forEach((root) => root.unmount());
        endpointRefs.current.forEach((popup) => popup.remove());
        endpointRefs.current = [];
        markerRoot?.unmount();
        markerRef.current?.remove();
        markerRef.current = null;
        map?.remove();
        mapRef.current = null;
      };
    }, []);

    useEffect(() => {
      if (mapRef.current) retryRef.current();
    }, [props.basemap]);

    useEffect(() => {
      const map = mapRef.current;
      if (!map?.getSource("flight-selected")) return;
      const data = flightGeoJson(props.records);
      for (const [id, geojson] of Object.entries(data))
        (map.getSource(`flight-${id}`) as GeoJSONSource | undefined)?.setData(
          geojson,
        );
      const selected = selectedGeoJson(
        props.selected,
        mapCoordinates(props.records)[0]?.[0],
      );
      (map.getSource("flight-selected") as GeoJSONSource).setData(selected);
      updateDroneMarker(map, markerRef.current, selected);
      updateEndpoints(map, endpointRefs.current, props.records, props.showPath);
      for (const id of ["flight-route"])
        if (map.getLayer(id))
          map.setLayoutProperty(
            id,
            "visibility",
            props.showPath ? "visible" : "none",
          );
      map
        .getCanvas()
        .setAttribute(
          "aria-label",
          `Interactive flight map. Selected record ${props.selected.id} at ${props.selected.time}.`,
        );
      const coordinate = selected.features[0]?.geometry.coordinates;
      if (
        coordinate &&
        !map.getBounds().contains([coordinate[0], coordinate[1]])
      )
        map.easeTo({ center: [coordinate[0], coordinate[1]], duration: 350 });
    }, [props.records, props.selected, props.showPath]);

    return (
      <div
        ref={container}
        className="drone-map absolute! inset-0 [&_.maplibregl-ctrl-attrib]:bg-card! [&_.maplibregl-ctrl-attrib]:text-foreground! [&_.maplibregl-ctrl-attrib_a]:text-foreground!"
        aria-label="MapLibre flight map"
      />
    );
  },
);
