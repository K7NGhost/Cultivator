import { invoke } from "@tauri-apps/api/core";
import type { StoredArtifactRecord } from "@/features/artifacts/model/types";
import type { ConversionMetadata, TelemetryPage, ImportResult } from "../types";

export type DroneFlight = Omit<StoredArtifactRecord, "payload"> & {
  payload: {
    metadata: ConversionMetadata;
    durationSeconds: number;
    maxAltitude: number | null;
    maxSpeed: number | null;
  };
};

export function listDroneFlights(caseDatabasePath: string) {
  return invoke<DroneFlight[]>("list_drone_flights", { caseDatabasePath });
}
export function openDroneFlight(caseDatabasePath: string, artifactId: string) {
  return invoke<ImportResult>("open_drone_flight", {
    caseDatabasePath,
    artifactId,
  });
}
export function droneTelemetryPage(
  caseDatabasePath: string,
  artifactId: string,
  page: number,
  query: string,
) {
  return invoke<TelemetryPage>("drone_telemetry_page", {
    caseDatabasePath,
    artifactId,
    page,
    query,
  });
}
