import type { StyleSpecification } from "maplibre-gl";

export type Basemap = "streets" | "satellite";

/** Shared by flight maps and artifact location previews. */
export function basemapStyle(basemap: Basemap): string | StyleSpecification {
  if (basemap === "streets") {
    return import.meta.env.VITE_MAP_STYLE_URL ||
      `https://tiles.openfreemap.org/styles/${document.documentElement.classList.contains("dark") ? "dark" : "positron"}`;
  }
  return {
    version: 8,
    sources: {
      "satellite-imagery": {
        type: "raster",
        tiles: ["https://services.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}"],
        tileSize: 256, maxzoom: 19,
        attribution: 'Imagery &copy; <a href="https://www.esri.com/" target="_blank" rel="noopener noreferrer">Esri</a> and its data providers',
      },
    },
    layers: [{ id: "satellite-imagery", type: "raster", source: "satellite-imagery", paint: { "raster-fade-duration": 0 } }],
  };
}

export function localStyle(): StyleSpecification {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 1;
  const context = canvas.getContext("2d");
  let background = "#20252b";
  if (context) {
    context.fillStyle = getComputedStyle(document.documentElement).getPropertyValue("--muted").trim();
    context.fillRect(0, 0, 1, 1);
    const [r, g, b] = context.getImageData(0, 0, 1, 1).data;
    background = `rgb(${r},${g},${b})`;
  }
  return { version: 8, sources: {}, layers: [{ id: "local-background", type: "background", paint: { "background-color": background } }] };
}
