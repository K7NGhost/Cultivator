import type { Basemap } from "@/lib/maps/mapStyles";
import type { MapDateFilter } from "../lib/dateFilter";
import { geometryBounds, renderCollection } from "../lib/mapFeatures";
import type { VisibleMapLayer } from "../types";
import { formatInstant, parseTimestamp } from "../../../lib/timeZone";

export type MapHtmlOptions = {
  title: string;
  layers: VisibleMapLayer[];
  dates: MapDateFilter;
  basemap: Basemap | "offline";
  exportedAt?: string;
  timeZone?: string;
};

export type MapHtmlAssets = { library: string; worker: string; mapCss: string; license: string; runtime: string; css: string };

function scriptJson(value: unknown) {
  return JSON.stringify(value).replace(/</g, "\\u003c").replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029");
}

function text(value: string) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** A portable snapshot. Source geometry and properties are never mutated. */
export function createMapHtml(options: MapHtmlOptions, assets: MapHtmlAssets): string {
  const collection = renderCollection(options.layers);
  if (!collection.features.length) throw new Error("There are no visible features to export.");
  const timeZone = options.timeZone ?? "UTC";
  const localTimes = new Map<string, string>();
  function collectTimes(value: unknown, depth = 0) {
    if (depth > 32) return;
    if (typeof value === "string") {
      if (localTimes.has(value)) return;
      const timestamp = parseTimestamp(value);
      if (timestamp?.basis === "zoned") localTimes.set(value, formatInstant(timestamp.time, timeZone));
    } else if (Array.isArray(value)) value.forEach((item) => collectTimes(item, depth + 1));
    else if (value && typeof value === "object") Object.values(value).forEach((item) => collectTimes(item, depth + 1));
  }
  collection.features.forEach((feature) => collectTimes(feature.properties));
  const snapshot = {
    title: options.title,
    exportedAt: options.exportedAt ?? new Date().toISOString(),
    dates: options.dates,
    timeZone,
    localTimes: Object.fromEntries(localTimes),
    basemap: options.basemap,
    bounds: geometryBounds(options.layers.flatMap((layer) => layer.collection.features.map((feature) => feature.geometry))),
    layers: options.layers.filter((layer) => layer.collection.features.length).map((layer) => ({
      id: layer.id, label: layer.label, source: layer.filePath, color: layer.color,
      features: layer.collection.features.length, format: layer.format, warnings: layer.warnings,
    })),
    collection,
  };
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${text(options.title)}</title>
<style>${assets.mapCss.replace(/<\/style/gi, "<\\/style")}\n${assets.css.replace(/<\/style/gi, "<\\/style")}</style>
</head><body>
<header><strong id="title"></strong><span id="summary"></span><button id="fit" type="button">Fit visible layers</button>
<label>Basemap <select id="basemap"><option value="streets">Streets</option><option value="satellite">Satellite</option><option value="offline">Offline background</option></select></label>
<button id="panels" type="button" aria-expanded="true">Details</button></header>
<div id="workspace"><aside id="details"><h2>Layers</h2><div id="layers"></div><p id="dates"></p>
<h2>Feature details</h2><div id="properties">Click a point, line or polygon to inspect it.</div>
<details><summary>Export information</summary><p id="exported"></p><p>Map data and viewer code are embedded. Online basemaps require internet access. The embedded data is not changed by layer toggles.</p>
<details><summary>MapLibre license</summary><pre id="license"></pre></details></details></aside>
<main id="map" aria-label="Exported interactive map"></main></div><footer id="status" role="status">Loading map…</footer>
<script type="application/json" id="map-data">${scriptJson(snapshot)}</script>
<script type="application/json" id="map-assets">${scriptJson({ library: assets.library, worker: assets.worker, license: assets.license })}</script>
<script type="module">${assets.runtime.replace(/<\/script/gi, "<\\/script")}</script>
</body></html>`;
}
