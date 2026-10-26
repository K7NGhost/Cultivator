export type FlightRecord = {
  id: number;
  time: string;
  elapsedSeconds?: number;
  latitude: number | null;
  longitude: number | null;
  coordinateSource?: string | null;
  coordinateTick?: number | null;
  coordinateOffset?: number | null;
  altitude: number | null;
  altitudeReference?: string | null;
  speed: number | null;
  battery: number | null;
  satellites: number | null;
  mode: string | null;
  tick?: number;
  sourceOffset?: number;
  batteryVoltage?: number | null;
  batteryCurrent?: number | null;
  batteryTemperature?: number | null;
  barometricAltitude?: number | null;
  gpsAltitudeMsl?: number | null;
  controllerRelativeAltitude?: number | null;
  gpsUtc?: string | null;
  gpsFixType?: number | null;
  gpsHorizontalAccuracy?: number | null;
  gpsVerticalAccuracy?: number | null;
};

export type FlightEvent = {
  time: string;
  elapsedSeconds?: number;
  title: string;
  detail: string;
  severity: string;
  record: number;
};

export type ConversionMetadata = {
  carving?: {
    imagePath: string;
    imageOffset: number;
    recoveredBytes: number;
    stoppedAtSizeLimit: boolean;
  } | null;
  id: string;
  sourceName: string;
  sourcePath: string;
  sourceBytes: number;
  sourceSha256: string;
  format: string;
  aircraft: string;
  firmwareDate: string;
  csvPath: string;
  csvSha256: string;
  rowCount: number;
  packetCount: number;
  rejectedPackets: number;
  skippedBytes: number;
  definitionCount: number;
  coordinateSource: string;
  routePointCount: number;
  routeDistanceMeters: number | null;
  unmappedRecordTypes: { id: number; payloadBytes: number; count: number }[];
  warnings: string[];
};

export type CsvPage = {
  columns: string[];
  rows: { values: string[]; telemetry: FlightRecord }[];
  total: number;
  page: number;
  pageSize: number;
};

export type ImportResult = {
  metadata: ConversionMetadata;
  page: CsvPage;
  route: FlightRecord[];
  events: FlightEvent[];
};
