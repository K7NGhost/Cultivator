import { getCaseDatabase } from "@/features/cases";
import { parseMapLayer } from "../lib/mapFeatures";
import type { MapLayerSummary } from "../types";

export async function listMapLayers(databasePath: string): Promise<MapLayerSummary[]> {
  const database = await getCaseDatabase(databasePath);
  const tables = await database.select<{ name: string }[]>("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'plugin_results'");
  if (!tables.length) return [];
  return database.select<MapLayerSummary[]>(`SELECT id, label, file_path AS filePath, datasource_id AS datasourceId,
    json_extract(payload, '$.format') AS format,
    json_extract(payload, '$.featureCount') AS featureCount,
    json_extract(payload, '$.positionCount') AS positionCount
    FROM plugin_results WHERE plugin_id = 'map-files' AND result_kind = 'map_layer'
    ORDER BY label COLLATE NOCASE, file_path, id`);
}

export async function loadMapLayer(databasePath: string, id: string) {
  const database = await getCaseDatabase(databasePath);
  const rows = await database.select<{ payload: string }[]>(
    "SELECT payload FROM plugin_results WHERE id = $1 AND plugin_id = 'map-files' AND result_kind = 'map_layer'", [id]);
  if (!rows[0]) throw new Error("This map layer no longer exists. Refresh the layer list.");
  return parseMapLayer(rows[0].payload);
}
