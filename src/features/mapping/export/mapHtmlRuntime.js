const snapshot = JSON.parse(document.getElementById("map-data").textContent);
const assets = JSON.parse(document.getElementById("map-assets").textContent);
const status = document.getElementById("status");
const shown = new Set(snapshot.layers.map((layer) => layer.id));
const payload = snapshot.collection;
const sourceId = "exported-features";
const featureLayers = ["export-points", "export-lines", "export-polygons"];
const selector = document.getElementById("basemap");
document.getElementById("title").textContent = snapshot.title;
document.getElementById("summary").textContent = `${payload.features.length.toLocaleString()} features · ${snapshot.layers.length} layers`;
document.getElementById("license").textContent = assets.license;
document.getElementById("exported").textContent = `Exported ${snapshot.exportedAt}`;
const dateDescription = snapshot.dates.from || snapshot.dates.to
  ? `Dates: ${snapshot.dates.from || "any start"} through ${snapshot.dates.to || "any end"} (inclusive).` : "All dates.";
const timeDescription = snapshot.dates.fromTime || snapshot.dates.toTime
  ? `Daily time: ${snapshot.dates.fromTime || "00:00:00"} through ${snapshot.dates.toTime || "23:59:59"} (inclusive; earlier end crosses midnight).` : "All times of day.";
document.getElementById("dates").textContent = `${dateDescription} ${timeDescription} Timezone: ${snapshot.timeZone || "UTC"}. Unknown timezones retain recorded clocks. Missing dates/times ${snapshot.dates.includeUndated ? "included" : "excluded when filtering"}.`;

function displayValue(value, depth = 0) {
  if (depth > 32) return value;
  if (typeof value === "string" && Object.prototype.hasOwnProperty.call(snapshot.localTimes || {}, value)) return snapshot.localTimes[value];
  if (Array.isArray(value)) return value.map((item) => displayValue(item, depth + 1));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, displayValue(item, depth + 1)]));
  return value;
}

let map;
let library;
let timer;
let offline = snapshot.basemap === "offline";
let fitted = false;
const fallbackStyle = { version: 8, sources: {}, layers: [{ id: "background", type: "background", paint: { "background-color": "#e5e9ed" } }] };
function style(kind) {
  if (kind === "offline") return fallbackStyle;
  return { version: 8, sources: { basemap: { type: "raster", tileSize: 256,
    tiles: kind === "satellite" ? ["https://services.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}"] : ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
    attribution: kind === "satellite" ? 'Imagery &copy; <a href="https://www.esri.com/">Esri</a>' : '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap contributors</a>' } },
    layers: [{ id: "basemap", source: "basemap", type: "raster" }] };
}
function visibleFeatures() { return payload.features.filter((feature) => shown.has(feature.properties._cvLayer)); }
function bounds(features) {
  const longitudes = [];
  let south = Infinity, north = -Infinity;
  function visit(geometry) {
    if (geometry.type === "GeometryCollection") { geometry.geometries.forEach(visit); return; }
    function walk(values) {
      if (!Array.isArray(values)) return;
      if (typeof values[0] === "number" && typeof values[1] === "number") {
        longitudes.push(((values[0] % 360) + 360) % 360);
        south = Math.min(south, values[1]); north = Math.max(north, values[1]);
      } else values.forEach(walk);
    }
    walk(geometry.coordinates);
  }
  features.forEach((feature) => visit(feature.geometry));
  if (!longitudes.length) return null;
  longitudes.sort((a, b) => a - b);
  let gap = -1, start = 0;
  longitudes.forEach((longitude, index) => {
    const next = index + 1 === longitudes.length ? longitudes[0] + 360 : longitudes[index + 1];
    if (next - longitude > gap) { gap = next - longitude; start = (index + 1) % longitudes.length; }
  });
  let west = longitudes[start], east = west + 360 - gap;
  if (west > 180) { west -= 360; east -= 360; }
  return [[west, south], [east, north]];
}
function fit() {
  const area = bounds(visibleFeatures());
  if (area) map.fitBounds(area, { padding: 55, maxZoom: 17, duration: 300 });
}
function update() {
  map.getSource(sourceId)?.setData({ type: "FeatureCollection", features: visibleFeatures() });
}
function showProperties(item) {
  const box = document.getElementById("properties");
  box.replaceChildren();
  const layer = snapshot.layers.find((layer) => layer.id === item.properties._cvLayer);
  const heading = document.createElement("strong");
  heading.textContent = item.properties.name || item.properties.title || item.geometry.type;
  box.append(heading);
  const list = document.createElement("dl");
  const values = { Layer: layer?.label, Source: layer?.source, Geometry: item.geometry.type };
  if (item.geometry.type === "Point") {
    values.Longitude = item.geometry.coordinates[0]; values.Latitude = item.geometry.coordinates[1];
    if (item.geometry.coordinates[2] !== undefined) values.Elevation = item.geometry.coordinates[2];
  }
  for (const [key, value] of [...Object.entries(values), ...Object.entries(item.properties).filter(([key]) => !key.startsWith("_cv"))]) {
    if (value === undefined) continue;
    const label = document.createElement("dt"), content = document.createElement("dd");
    const display = displayValue(value);
    label.textContent = key; content.textContent = typeof display === "string" ? display : JSON.stringify(display, null, 2);
    if (typeof value === "string") content.title = value;
    list.append(label, content);
  }
  box.append(list);
  document.getElementById("details").hidden = false;
  document.getElementById("panels").setAttribute("aria-expanded", "true");
  map.resize();
}
for (const layer of snapshot.layers) {
  const label = document.createElement("label"), input = document.createElement("input"), swatch = document.createElement("i"), text = document.createElement("span"), count = document.createElement("small");
  input.type = "checkbox"; input.checked = true;
  input.addEventListener("change", () => { input.checked ? shown.add(layer.id) : shown.delete(layer.id); if (map) update(); });
  swatch.style.backgroundColor = /^#[0-9a-f]{6}$/i.test(layer.color) ? layer.color : "#f97316";
  text.textContent = layer.label; count.textContent = `${layer.features.toLocaleString()} features · ${layer.format.toUpperCase()}`;
  text.append(count); label.append(input, swatch, text); document.getElementById("layers").append(label);
}
document.getElementById("panels").addEventListener("click", () => {
  const details = document.getElementById("details"); details.hidden = !details.hidden;
  document.getElementById("panels").setAttribute("aria-expanded", String(!details.hidden)); map?.resize();
});
document.getElementById("fit").addEventListener("click", () => { if (map) fit(); });

try {
  const libraryUrl = URL.createObjectURL(new Blob([assets.library], { type: "text/javascript" }));
  // Classic workers support local-file origins. The .cjs hint tells MapLibre
  // to create a classic worker for this embedded IIFE bundle.
  const workerUrl = `data:text/javascript;charset=utf-8,${encodeURIComponent(assets.worker)}#.cjs`;
  library = await import(libraryUrl);
  library.setWorkerUrl(workerUrl);
  selector.value = snapshot.basemap;
  // The offline background is available immediately. Network tiles are added
  // only after the embedded geometry has been installed.
  map = new library.Map({ container: "map", style: fallbackStyle, center: [0,0], zoom: 1, attributionControl: { compact: true } });
  map.addControl(new library.NavigationControl(), "top-right");
  map.addControl(new library.ScaleControl({ unit: "metric" }), "bottom-left");
  map.on("style.load", () => {
    map.addSource(sourceId, { type: "geojson", data: { type: "FeatureCollection", features: visibleFeatures() } });
    map.addLayer({ id: "export-polygons", type: "fill", source: sourceId, filter: ["==",["geometry-type"],"Polygon"], paint: { "fill-color": ["get","_cvColor"], "fill-opacity": 0.2 } });
    map.addLayer({ id: "export-lines", type: "line", source: sourceId, filter: ["in",["geometry-type"],["literal",["LineString","Polygon"]]], layout: { "line-cap":"round", "line-join":"round" }, paint: { "line-color":["get","_cvColor"], "line-width":3 } });
    map.addLayer({ id: "export-points", type: "circle", source: sourceId, filter: ["==",["geometry-type"],"Point"], paint: { "circle-color":["get","_cvColor"], "circle-radius":5, "circle-stroke-color":"#ffffff", "circle-stroke-width":1.5 } });
    if (!fitted) { fit(); fitted = true; }
    status.textContent = offline ? "Offline background. Embedded map features remain available." : "Map data is embedded. Basemap imagery requires internet access.";
  });
  function loadBasemap() {
    clearTimeout(timer); offline = selector.value === "offline";
    map.setStyle(style(selector.value), { diff: false });
    if (!offline) timer = setTimeout(() => { if (!map.isStyleLoaded()) { offline = true; map.setStyle(fallbackStyle, { diff: false }); } }, 15000);
  }
  map.once("load", () => { if (selector.value !== "offline") loadBasemap(); });
  selector.addEventListener("change", loadBasemap);
  map.on("error", (event) => {
    if (event.sourceId === sourceId) {
      status.textContent = `Cannot display embedded geometry: ${String(event.error?.message || event.error)}`;
      return;
    }
    if (event.sourceId !== "basemap") {
      status.textContent = `Map error: ${String(event.error?.message || event.error)}`;
      return;
    }
    if (!offline) { offline = true; clearTimeout(timer); map.setStyle(fallbackStyle, { diff: false }); }
    status.textContent = "Basemap unavailable. Embedded map features remain visible.";
  });
  map.on("click", (event) => {
    if (!featureLayers.every((id) => map.getLayer(id))) return;
    const hit = map.queryRenderedFeatures(event.point, { layers: featureLayers })[0];
    const item = hit && payload.features.find((item) => item.properties._cvKey === hit.properties._cvKey);
    if (item) showProperties(item);
  });
  new ResizeObserver(() => map.resize()).observe(document.getElementById("map"));
} catch (error) {
  status.textContent = `Cannot initialize the interactive map. Use a browser with WebGL support. ${String(error)}`;
}
