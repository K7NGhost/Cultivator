import { invoke } from "@tauri-apps/api/core";
import { createMapHtml, type MapHtmlOptions } from "./createMapHtml";
import library from "maplibre-gl/dist/maplibre-gl.mjs?raw";
import worker from "virtual:map-export-worker";
import mapCss from "maplibre-gl/dist/maplibre-gl.css?raw";
import license from "./maplibre-license.txt?raw";
import runtime from "./mapHtmlRuntime.js?raw";
import css from "./mapHtml.css?raw";

export async function exportMapHtml(caseDatabasePath: string, options: MapHtmlOptions) {
  const html = createMapHtml(options, { library, worker, mapCss, license, runtime, css });
  return invoke<string | null>("export_map_html", { caseDatabasePath, html });
}
