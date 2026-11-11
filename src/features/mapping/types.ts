import type { Feature, FeatureCollection, Geometry } from "geojson";

export type MapLayerSummary = {
  id: string;
  label: string;
  filePath: string;
  datasourceId: string;
  format: string;
  featureCount: number;
  positionCount: number;
};

export type MapLayerData = {
  collection: FeatureCollection<Geometry>;
  warnings: string[];
};

export type VisibleMapLayer = MapLayerSummary & MapLayerData & {
  color: string;
  /** Original source indices, retained when a view filters the feature array. */
  featureIndices?: number[];
  totalFeatureCount?: number;
};
export type FeatureSelection = { layerId: string; index: number };
export type MapFeature = Feature<Geometry>;
