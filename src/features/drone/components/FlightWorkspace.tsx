import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { FlightMap } from "./FlightMap";
import { TelemetryChart } from "./TelemetryChart";
import {
  DroneTelemetryTable,
  type TelemetryTableProps,
} from "./DroneTelemetryTable";
import { finiteLabel } from "../lib/telemetry";
import type { ImportResult } from "../types";

export function FlightWorkspace({
  active,
  detail,
  ...tableProps
}: TelemetryTableProps & { active: boolean; detail: ImportResult }) {
  const { selected, onSelect } = tableProps;
  const { metadata, route, samples, events } = detail;
  const properties: [string, string][] = [
    ["Sample", `#${selected.id}`],
    ["Elapsed", selected.time],
    ["GPS UTC", selected.gpsUtc ?? "—"],
    ["Mode", selected.mode ?? "—"],
    ["Latitude", finiteLabel(selected.latitude, 7)],
    ["Longitude", finiteLabel(selected.longitude, 7)],
    ["Altitude", `${finiteLabel(selected.altitude, 3)} m`],
    [
      "Altitude reference",
      selected.altitudeReference?.replace(/_/g, " ") ?? "—",
    ],
    ["Ground speed", `${finiteLabel(selected.speed, 3)} m/s`],
    [
      "GPS fix",
      selected.gpsFixType == null
        ? "—"
        : selected.gpsFixType === 0
          ? "No fix"
          : selected.gpsFixType <= 3
            ? `${selected.gpsFixType}D`
            : `Type ${selected.gpsFixType}`,
    ],
    ["Satellites", finiteLabel(selected.satellites, 0)],
    [
      "Horizontal accuracy",
      `${finiteLabel(selected.gpsHorizontalAccuracy, 2)} m`,
    ],
    ["Vertical accuracy", `${finiteLabel(selected.gpsVerticalAccuracy, 2)} m`],
    ["Battery", `${finiteLabel(selected.battery, 0)}%`],
    ["Voltage", `${finiteLabel(selected.batteryVoltage, 2)} V`],
    ["Position source", selected.coordinateSource ?? "—"],
    ["Coordinate offset", String(selected.coordinateOffset ?? "—")],
    ["Sample offset", String(selected.sourceOffset ?? "—")],
  ];
  return (
    <>
      <div className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-muted-foreground">
        <span className="min-w-0 truncate" title={metadata.sourcePath}>
          {metadata.sourceName}
        </span>
        <span className="ml-auto shrink-0">
          {metadata.aircraft} · {metadata.format}
        </span>
      </div>
      <div className="flex min-h-0 flex-1">
        <Tabs defaultValue="map" className="min-h-0 min-w-0 flex-1 gap-0">
          <div className="shrink-0 border-b px-2">
            <TabsList variant="line" className="h-8">
              <TabsTrigger value="map" className="rounded-none text-xs">
                Flight path
              </TabsTrigger>
              <TabsTrigger value="telemetry" className="rounded-none text-xs">
                Telemetry
              </TabsTrigger>
              <TabsTrigger value="events" className="rounded-none text-xs">
                Events ({events.length})
              </TabsTrigger>
              <TabsTrigger value="source" className="rounded-none text-xs">
                Source / parser notes
              </TabsTrigger>
            </TabsList>
          </div>
          <TabsContent
            value="map"
            className="min-h-0 flex-col overflow-auto data-[state=active]:flex"
          >
            {active && (
              <FlightMap
                records={route}
                selected={selected}
                onSelect={onSelect}
                actualPointCount={metadata.routePointCount}
              />
            )}
            <div className="grid shrink-0 border-t md:grid-cols-2">
              <TelemetryChart
                records={samples}
                selected={selected}
                field="altitude"
                title={
                  metadata.coordinateSource === "gps"
                    ? "GPS altitude (MSL)"
                    : "Altitude"
                }
                unit="m"
                color="var(--primary)"
                onSelect={onSelect}
              />
              <TelemetryChart
                records={samples}
                selected={selected}
                field="speed"
                title="Ground speed"
                unit="m/s"
                color="var(--foreground)"
                onSelect={onSelect}
              />
            </div>
          </TabsContent>
          <TabsContent value="telemetry" className="min-h-0 overflow-hidden">
            <DroneTelemetryTable {...tableProps} />
          </TabsContent>
          <TabsContent value="events" className="min-h-0 overflow-auto">
            <Table className="text-xs">
              <TableHeader>
                <TableRow>
                  <TableHead className="h-8">Elapsed</TableHead>
                  <TableHead className="h-8">Event</TableHead>
                  <TableHead className="h-8">Details</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {events.map((event, i) => (
                  <TableRow key={i}>
                    <TableCell className="py-1 font-mono">
                      <Button
                        variant="link"
                        className="h-auto p-0 text-xs"
                        onClick={() => {
                          const row = route.find(
                            (item) => item.id >= event.record,
                          );
                          if (row) onSelect(row);
                        }}
                      >
                        {event.time}
                      </Button>
                    </TableCell>
                    <TableCell className="py-1">{event.title}</TableCell>
                    <TableCell className="whitespace-normal break-words py-1 text-muted-foreground">
                      {event.detail}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {!events.length && (
              <p className="p-2 text-muted-foreground">
                No controller events decoded.
              </p>
            )}
          </TabsContent>
          <TabsContent value="source" className="min-h-0 overflow-auto p-2">
            <dl className="grid grid-cols-[130px_minmax(0,1fr)] gap-x-2 gap-y-2 text-xs">
              {metadata.carving && (
                <>
                  <dt className="text-muted-foreground">Original image</dt>
                  <dd className="break-all font-mono">
                    {metadata.carving.imagePath}
                  </dd>
                  <dt className="text-muted-foreground">
                    Decoded image offset
                  </dt>
                  <dd className="font-mono">
                    0x{metadata.carving.imageOffset.toString(16)}
                  </dd>
                  <dt className="text-muted-foreground">Carved bytes</dt>
                  <dd>
                    {metadata.carving.recoveredBytes.toLocaleString()}{" "}
                    (contiguous recovery
                    {metadata.carving.stoppedAtSizeLimit
                      ? "; size limit reached"
                      : ""}
                    )
                  </dd>
                </>
              )}
              <dt className="text-muted-foreground">Source DAT</dt>
              <dd className="break-all font-mono">{metadata.sourcePath}</dd>
              <dt className="text-muted-foreground">Source SHA-256</dt>
              <dd className="break-all font-mono">{metadata.sourceSha256}</dd>
              {metadata.csvSha256 && (
                <>
                  <dt className="text-muted-foreground">CSV SHA-256</dt>
                  <dd className="break-all font-mono">{metadata.csvSha256}</dd>
                </>
              )}
              <dt className="text-muted-foreground">Telemetry rows</dt>
              <dd>{metadata.rowCount.toLocaleString()}</dd>
              <dt className="text-muted-foreground">
                Valid / rejected packets
              </dt>
              <dd>
                {metadata.packetCount.toLocaleString()} /{" "}
                {metadata.rejectedPackets.toLocaleString()}
              </dd>
            </dl>
            <div className="mt-3 border-t pt-2">
              <p className="font-medium">Parser notes</p>
              {metadata.warnings.length ? (
                <ul className="mt-2 list-disc space-y-1 pl-4 text-muted-foreground">
                  {metadata.warnings.map((note) => (
                    <li key={note}>{note}</li>
                  ))}
                </ul>
              ) : (
                <p className="mt-1 text-muted-foreground">
                  No parser warnings.
                </p>
              )}
            </div>
          </TabsContent>
        </Tabs>
        <aside
          className="w-56 shrink-0 overflow-auto border-l"
          aria-label="Selected sample"
        >
          <div className="flex h-8 items-center border-b px-2 font-medium text-muted-foreground">
            Selected sample
          </div>
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-2 p-2 text-[10px]">
            {properties.map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="text-muted-foreground">{label}</dt>
                <dd className="break-words text-right font-mono">{value}</dd>
              </div>
            ))}
          </dl>
          {selected.gpsFixType === 0 && (
            <p
              role="status"
              className="border-t p-2 text-[10px] text-muted-foreground"
            >
              Receiver has no fix. Coordinates are the receiver's reported
              estimate after acquisition.
            </p>
          )}
        </aside>
      </div>
      <div className="flex h-7 shrink-0 items-center gap-2 border-t px-2 text-[10px] text-muted-foreground">
        <span>{metadata.rowCount.toLocaleString()} telemetry rows</span>
        <span>·</span>
        <span>
          {metadata.routePointCount.toLocaleString()} receiver positions
        </span>
        <span className="ml-auto">CSV integrity verified</span>
      </div>
    </>
  );
}
