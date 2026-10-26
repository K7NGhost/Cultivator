import { useCallback, useEffect, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  ArrowDownToLine,
  ArrowUpRight,
  Battery,
  ChevronLeft,
  ChevronRight,
  Clock3,
  Drone,
  FileSearch,
  Gauge,
  LoaderCircle,
  MapPin,
  Plug,
  Radio,
  RefreshCw,
  Route,
  Search,
  ShieldCheck,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { useCases } from "@/features/cases";
import type { CaseRecord } from "@/features/cases/model/types";
import { FlightMap } from "../components/FlightMap";
import { TelemetryChart } from "../components/TelemetryChart";
import { elapsedLabel, finiteLabel } from "../lib/telemetry";
import {
  droneCsvPage,
  listDroneFlights,
  openDroneFlight,
  type DroneFlight,
} from "../services/droneRepository";
import type { CsvPage, FlightRecord, ImportResult } from "../types";
import "../drone.css";

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export function DronePage() {
  const { activeCase } = useCases();
  if (!activeCase)
    return (
      <div className="grid h-full place-items-center p-8">
        <div className="max-w-sm text-center">
          <Drone className="mx-auto size-12 text-muted-foreground/40" />
          <h1 className="mt-5 text-xl font-semibold">Drone analysis</h1>
          <p className="mt-2 text-sm text-muted-foreground">
            Open a case to explore DAT flight logs, telemetry, and recorded
            flight paths.
          </p>
          <Button asChild className="mt-5">
            <Link to="/case">Open a case</Link>
          </Button>
        </div>
      </div>
    );
  return (
    <DroneWorkspace key={activeCase.databasePath} caseRecord={activeCase} />
  );
}

function DroneWorkspace({ caseRecord }: { caseRecord: CaseRecord }) {
  const active = useLocation().pathname === "/drone";
  const [flights, setFlights] = useState<DroneFlight[]>([]);
  const [flightId, setFlightId] = useState("");
  const [detail, setDetail] = useState<ImportResult | null>(null);
  const [selected, select] = useState<FlightRecord | null>(null);
  const [table, setTable] = useState<CsvPage | null>(null);
  const [page, setPage] = useState(0);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("");
  const [loading, setLoading] = useState(true);
  const [opening, setOpening] = useState(false);
  const [paging, setPaging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showAllEvents, setShowAllEvents] = useState(false);
  const flight = flights.find((item) => item.id === flightId);
  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await listDroneFlights(caseRecord.databasePath);
      setFlights(next);
      setFlightId((previous) =>
        next.some((item) => item.id === previous)
          ? previous
          : (next[0]?.id ?? ""),
      );
    } catch (caught) {
      setError(message(caught));
    } finally {
      setLoading(false);
    }
  }, [caseRecord.databasePath]);
  useEffect(() => {
    void refresh();
    const listener = listen("plugin-run-finished", () => {
      void refresh();
    });
    return () => {
      void listener.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [refresh]);
  useEffect(() => {
    let disposed = false;
    setDetail(null);
    select(null);
    setTable(null);
    setPage(0);
    setQuery("");
    setFilter("");
    setShowAllEvents(false);
    if (!flightId) {
      setOpening(false);
      return;
    }
    setOpening(true);
    setError(null);
    void openDroneFlight(caseRecord.databasePath, flightId)
      .then((next) => {
        if (disposed) return;
        setDetail(next);
        setTable(next.page);
        select(next.route[0] ?? next.page.rows[0]?.telemetry ?? null);
      })
      .catch((caught) => {
        if (!disposed) setError(message(caught));
      })
      .finally(() => {
        if (!disposed) setOpening(false);
      });
    return () => {
      disposed = true;
    };
  }, [caseRecord.databasePath, flightId]);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setPage(0);
      setFilter(query);
    }, 300);
    return () => window.clearTimeout(timer);
  }, [query]);
  useEffect(() => {
    if (!detail) return;
    let disposed = false;
    setPaging(true);
    void droneCsvPage(caseRecord.databasePath, flightId, page, filter)
      .then((next) => {
        if (!disposed) setTable(next);
      })
      .catch((caught) => {
        if (!disposed) setError(message(caught));
      })
      .finally(() => {
        if (!disposed) setPaging(false);
      });
    return () => {
      disposed = true;
    };
  }, [caseRecord.databasePath, flightId, detail, page, filter]);
  async function exportCsv() {
    try {
      await invoke("export_drone_csv", {
        caseDatabasePath: caseRecord.databasePath,
        artifactId: flightId,
      });
    } catch (caught) {
      setError(message(caught));
    }
  }
  return (
    <div
      data-drone-active={active ? "true" : "false"}
      className="flex h-full min-h-0 min-w-0 flex-col bg-muted/15"
    >
      <header className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b bg-background px-6 py-4">
        <div className="flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl border border-teal-500/20 bg-teal-500/10 text-teal-600 dark:text-teal-400">
            <Drone className="size-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-lg font-semibold tracking-tight">
                Drone analysis
              </h1>
              <span className="rounded-full border px-2 py-0.5 text-[10px] text-muted-foreground">
                DJI DAT
              </span>
            </div>
            <p className="text-xs text-muted-foreground">
              Flight paths, telemetry, and controller events
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => void refresh()}
            disabled={loading}
            aria-label="Refresh flights"
          >
            <RefreshCw
              className={`size-3.5 ${loading ? "animate-spin" : ""}`}
            />
          </Button>
          <Button size="sm" variant="outline" asChild>
            <Link to="/plugins">
              <Plug className="size-3.5" />
              Run plugin
              <ArrowUpRight className="size-3" />
            </Link>
          </Button>
          {flight && (
            <Button size="sm" onClick={() => void exportCsv()}>
              <ArrowDownToLine className="size-3.5" />
              Export CSV
            </Button>
          )}
        </div>
      </header>
      {error && (
        <div
          role="alert"
          className="flex items-center justify-between gap-3 border-b border-destructive/20 bg-destructive/5 px-6 py-3 text-xs text-destructive"
        >
          <span>{error}</span>
          <Button size="sm" variant="ghost" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      )}
      {!flights.length ? (
        <div className="grid flex-1 place-items-center p-8">
          <div className="max-w-md text-center">
            <div className="mx-auto grid size-20 place-items-center rounded-3xl border bg-card shadow-sm">
              <Drone className="size-9 text-teal-600/70" />
            </div>
            <h2 className="mt-6 text-xl font-semibold">
              Every flight tells a story
            </h2>
            <p className="mt-3 text-sm leading-relaxed text-muted-foreground">
              Add a datasource containing DAT logs, then run{" "}
              <strong className="font-medium text-foreground">
                Drone Telemetry
              </strong>{" "}
              from Plugins. Decoded flights will appear here with their recorded
              routes and telemetry.
            </p>
            <Button asChild className="mt-6">
              <Link to="/plugins">
                <Plug className="size-4" />
                Open plugins
              </Link>
            </Button>
            <p className="mt-4 text-[11px] text-muted-foreground">
              Supported DJI DAT V1/V3 logs · Encrypted LOGH logs are unsupported
            </p>
            {loading && (
              <LoaderCircle className="mx-auto mt-5 size-5 animate-spin text-muted-foreground" />
            )}
          </div>
        </div>
      ) : (
        <div className="grid min-h-0 flex-1 lg:grid-cols-[220px_minmax(0,1fr)]">
          <aside className="hidden min-h-0 flex-col border-r bg-background lg:flex">
            <div className="flex items-center justify-between border-b px-4 py-3 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground">
              <span>Flight library</span>
              <span>{flights.length}</span>
            </div>
            <div className="flex-1 overflow-auto p-2">
              {flights.map((item) => (
                <button
                  key={item.id}
                  onClick={() => setFlightId(item.id)}
                  className={`mb-1 w-full rounded-lg border p-3 text-left transition-colors ${item.id === flightId ? "border-teal-500/35 bg-teal-500/8" : "border-transparent hover:bg-muted"}`}
                >
                  <div className="flex items-center gap-2">
                    <Drone
                      className={`size-3.5 shrink-0 ${item.id === flightId ? "text-teal-600" : "text-muted-foreground"}`}
                    />
                    <span
                      className="truncate text-xs font-semibold"
                      title={item.label}
                    >
                      {item.label}
                    </span>
                  </div>
                  <p className="mt-2 truncate text-[10px] text-muted-foreground">
                    {item.payload.metadata.aircraft}
                  </p>
                  <div className="mt-3 flex items-center justify-between text-[10px] text-muted-foreground">
                    <span className="font-mono">
                      {elapsedLabel(item.payload.durationSeconds)}
                    </span>
                    <span>
                      {item.payload.metadata.routePointCount.toLocaleString()}{" "}
                      fixes
                    </span>
                  </div>
                </button>
              ))}
            </div>
            <div className="border-t p-3 text-[10px] text-muted-foreground">
              <ShieldCheck className="mb-1 size-4 text-teal-600" />
              Source hashes retained with each conversion
            </div>
          </aside>
          <div className="min-h-0 min-w-0 overflow-auto p-4 xl:p-6">
            <select
              aria-label="Select flight"
              className="mb-4 w-full rounded-md border bg-background p-2 text-sm lg:hidden"
              value={flightId}
              onChange={(e) => setFlightId(e.target.value)}
            >
              {flights.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
            {opening ? (
              <div className="grid min-h-80 place-items-center text-sm text-muted-foreground">
                <div className="flex items-center gap-2">
                  <LoaderCircle className="size-4 animate-spin" />
                  Verifying and opening flight…
                </div>
              </div>
            ) : detail &&
              selected &&
              flight &&
              detail.metadata.id === flight.payload.metadata.id ? (
              <div className="mx-auto max-w-[1800px] space-y-4">
                <div className="flex items-end justify-between gap-3">
                  <div className="min-w-0">
                    <p className="text-[10px] font-medium uppercase tracking-widest text-teal-600 dark:text-teal-400">
                      Flight overview
                    </p>
                    <h2
                      className="mt-1 truncate text-lg font-semibold"
                      title={flight.filePath}
                    >
                      {detail.metadata.sourceName}
                    </h2>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {detail.metadata.aircraft} · {detail.metadata.format} ·{" "}
                      {detail.metadata.rowCount.toLocaleString()} telemetry rows
                    </p>
                  </div>
                  <span className="flex shrink-0 items-center gap-1.5 rounded-full bg-teal-500/10 px-2.5 py-1 text-[10px] font-medium text-teal-700 dark:text-teal-300">
                    <ShieldCheck className="size-3" />
                    CSV verified
                  </span>
                </div>
                <div className="grid grid-cols-2 gap-3 xl:grid-cols-4">
                  <Stat
                    icon={Clock3}
                    label="Duration"
                    value={elapsedLabel(flight.payload.durationSeconds)}
                  />
                  <Stat
                    icon={Route}
                    label="Ground distance"
                    value={
                      detail.metadata.routeDistanceMeters == null
                        ? "—"
                        : `${finiteLabel(detail.metadata.routeDistanceMeters / 1000, 2)} km`
                    }
                  />
                  <Stat
                    icon={MapPin}
                    label="Peak decoded altitude"
                    value={`${finiteLabel(flight.payload.maxAltitude)} m`}
                  />
                  <Stat
                    icon={Gauge}
                    label="Maximum speed"
                    value={`${finiteLabel(flight.payload.maxSpeed)} m/s`}
                  />
                </div>
                {detail.metadata.warnings.length > 0 && (
                  <details className="rounded-lg border border-amber-500/25 bg-amber-500/5 px-3 py-2 text-xs">
                    <summary className="cursor-pointer font-medium text-amber-700 dark:text-amber-300">
                      Parser notes · {detail.metadata.warnings.length}
                    </summary>
                    <ul className="mt-2 list-disc space-y-1.5 pl-4 text-muted-foreground">
                      {detail.metadata.warnings.map((warning) => (
                        <li key={warning}>{warning}</li>
                      ))}
                    </ul>
                  </details>
                )}
                <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_250px]">
                  {active && (
                    <FlightMap
                      key={flightId}
                      records={detail.route}
                      selected={selected}
                      onSelect={select}
                      actualPointCount={detail.metadata.routePointCount}
                    />
                  )}
                  <aside className="flex flex-col rounded-xl border bg-card p-4 shadow-sm">
                    <div className="flex items-center justify-between">
                      <h3 className="text-xs font-semibold">Selected sample</h3>
                      <span className="font-mono text-[10px] text-muted-foreground">
                        #{selected.id}
                      </span>
                    </div>
                    <div className="my-5 flex items-center gap-2">
                      <span className="size-2 rounded-full bg-teal-500" />
                      <span className="text-sm font-medium">
                        {selected.mode ?? "Mode unavailable"}
                      </span>
                    </div>
                    <div className="grid grid-cols-2 gap-x-3 gap-y-5">
                      <Instrument
                        icon={Gauge}
                        label="Speed"
                        value={`${finiteLabel(selected.speed)} m/s`}
                      />
                      <Instrument
                        icon={Battery}
                        label="Battery"
                        value={`${finiteLabel(selected.battery, 0)}%`}
                      />
                      <Instrument
                        icon={Radio}
                        label="Satellites"
                        value={finiteLabel(selected.satellites, 0)}
                      />
                      <Instrument
                        icon={Activity}
                        label="Voltage"
                        value={`${finiteLabel(selected.batteryVoltage, 2)} V`}
                      />
                    </div>
                    <div className="mt-5 space-y-2 border-t pt-4 text-[10px] text-muted-foreground">
                      <div className="flex justify-between">
                        <span>Position source</span>
                        <span className="font-mono uppercase text-foreground">
                          {selected.coordinateSource ?? "None"}
                        </span>
                      </div>
                      <div className="flex justify-between">
                        <span>Coordinate byte offset</span>
                        <span className="font-mono text-foreground">
                          {selected.coordinateOffset ?? "—"}
                        </span>
                      </div>
                      <div className="flex justify-between">
                        <span>Sample byte offset</span>
                        <span className="font-mono text-foreground">
                          {selected.sourceOffset ?? "—"}
                        </span>
                      </div>
                      <p className="break-words pt-2 leading-relaxed">
                        Altitude reference:{" "}
                        {selected.altitudeReference?.replace(/_/g, " ") ??
                          "unavailable"}
                      </p>
                    </div>
                    <div className="mt-auto border-t pt-4">
                      <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
                        Elapsed time
                      </p>
                      <p className="mt-1 font-mono text-lg">{selected.time}</p>
                    </div>
                  </aside>
                </div>
                <div className="grid gap-3 md:grid-cols-2">
                  <TelemetryChart
                    records={detail.route}
                    selected={selected}
                    field="altitude"
                    title="Altitude"
                    unit="m"
                    color="#14b8a6"
                    onSelect={select}
                  />
                  <TelemetryChart
                    records={detail.route}
                    selected={selected}
                    field="speed"
                    title="Speed"
                    unit="m/s"
                    color="#3b82f6"
                    onSelect={select}
                  />
                </div>
                <section
                  className="overflow-hidden rounded-xl border bg-card shadow-sm"
                  aria-label="Decoded telemetry"
                >
                  <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
                    <div className="flex items-center gap-2 text-xs font-semibold">
                      <FileSearch className="size-4 text-muted-foreground" />
                      Telemetry records{" "}
                      <span className="font-normal text-muted-foreground">
                        {table?.total.toLocaleString() ?? 0}
                      </span>
                      {paging && (
                        <LoaderCircle className="size-3 animate-spin" />
                      )}
                    </div>
                    <label className="flex items-center gap-2 rounded-md border bg-background px-2 py-1">
                      <Search className="size-3 text-muted-foreground" />
                      <input
                        aria-label="Search telemetry"
                        value={query}
                        onChange={(e) => setQuery(e.target.value)}
                        placeholder="Search decoded values…"
                        className="w-44 bg-transparent text-xs outline-none"
                      />
                    </label>
                  </div>
                  <div className="max-h-[330px] overflow-auto">
                    <table className="w-full border-collapse whitespace-nowrap text-left font-mono text-[10px]">
                      <thead className="sticky top-0 z-10 bg-muted">
                        <tr>
                          {table?.columns.map((column) => (
                            <th
                              key={column}
                              className="border-b px-3 py-2 font-medium text-muted-foreground"
                            >
                              {column}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {table?.rows.map((row) => (
                          <tr
                            key={row.telemetry.id}
                            onClick={() => select(row.telemetry)}
                            aria-selected={row.telemetry.id === selected.id}
                            className={`cursor-pointer border-b last:border-0 ${row.telemetry.id === selected.id ? "bg-teal-500/10" : "hover:bg-muted/60"}`}
                          >
                            {row.values.map((value, index) => (
                              <td key={index} className="px-3 py-2">
                                {value || (
                                  <span className="text-muted-foreground/45">
                                    —
                                  </span>
                                )}
                              </td>
                            ))}
                          </tr>
                        ))}
                      </tbody>
                    </table>
                    {!table?.rows.length && (
                      <p className="p-8 text-center text-xs text-muted-foreground">
                        No matching telemetry records
                      </p>
                    )}
                  </div>
                  <div className="flex items-center justify-between border-t px-4 py-2 text-[10px] text-muted-foreground">
                    <span>
                      Page {page + 1} of{" "}
                      {Math.max(1, Math.ceil((table?.total ?? 0) / 100))} · 100
                      rows per page
                    </span>
                    <div className="flex gap-1">
                      <Button
                        size="icon"
                        variant="ghost"
                        className="size-7"
                        disabled={page === 0 || paging}
                        aria-label="Previous telemetry page"
                        onClick={() => setPage((p) => p - 1)}
                      >
                        <ChevronLeft className="size-3.5" />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        className="size-7"
                        disabled={
                          (page + 1) * 100 >= (table?.total ?? 0) || paging
                        }
                        aria-label="Next telemetry page"
                        onClick={() => setPage((p) => p + 1)}
                      >
                        <ChevronRight className="size-3.5" />
                      </Button>
                    </div>
                  </div>
                </section>
                <section className="rounded-xl border bg-card p-4">
                  <div className="flex justify-between">
                    <h3 className="text-xs font-semibold">
                      Controller events{" "}
                      <span className="ml-1 font-normal text-muted-foreground">
                        {detail.events.length}
                      </span>
                    </h3>
                    {detail.events.length > 6 && (
                      <button
                        className="text-[10px] text-teal-600"
                        onClick={() => setShowAllEvents((v) => !v)}
                      >
                        {showAllEvents ? "Show fewer" : "Show all"}
                      </button>
                    )}
                  </div>
                  <div className="mt-3 max-h-72 space-y-0 overflow-auto">
                    {(showAllEvents
                      ? detail.events
                      : detail.events.slice(0, 6)
                    ).map((event, index) => (
                      <button
                        key={index}
                        className="flex w-full gap-3 border-l border-teal-500/25 py-2 pl-3 text-left hover:bg-muted/50"
                        onClick={() => {
                          const row = detail.route.find(
                            (item) => item.id >= event.record,
                          );
                          if (row) select(row);
                        }}
                      >
                        <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                          {event.time}
                        </span>
                        <div>
                          <p className="text-xs font-medium">{event.title}</p>
                          <p className="mt-1 break-words text-[11px] text-muted-foreground">
                            {event.detail}
                          </p>
                        </div>
                      </button>
                    ))}
                    {detail.events.length === 0 && (
                      <p className="py-2 text-xs text-muted-foreground">
                        No controller events decoded in this log.
                      </p>
                    )}
                  </div>
                </section>
                <details className="rounded-lg border bg-card px-4 py-3 text-xs">
                  <summary className="cursor-pointer text-muted-foreground">
                    Source and conversion provenance
                  </summary>
                  <dl className="mt-3 grid gap-2 text-[11px]">
                    <dt className="text-muted-foreground">Source DAT</dt>
                    <dd className="break-all font-mono">{flight.filePath}</dd>
                    <dt className="text-muted-foreground">Source SHA-256</dt>
                    <dd className="break-all font-mono">
                      {detail.metadata.sourceSha256}
                    </dd>
                    <dt className="text-muted-foreground">
                      Telemetry CSV SHA-256
                    </dt>
                    <dd className="break-all font-mono">
                      {detail.metadata.csvSha256}
                    </dd>
                    <dt className="text-muted-foreground">
                      Decoded / rejected packet candidates
                    </dt>
                    <dd>
                      {detail.metadata.packetCount.toLocaleString()} /{" "}
                      {detail.metadata.rejectedPackets.toLocaleString()}
                    </dd>
                  </dl>
                </details>
              </div>
            ) : (
              !error && (
                <p className="p-8 text-center text-sm text-muted-foreground">
                  Select a flight to inspect its telemetry.
                </p>
              )
            )}
          </div>
        </div>
      )}
    </div>
  );
}

type MetricIcon = typeof Clock3;
function Stat({
  icon: Icon,
  label,
  value,
}: {
  icon: MetricIcon;
  label: string;
  value: string;
}) {
  return (
    <div className="rounded-xl border bg-card px-4 py-3 shadow-sm">
      <div className="flex items-center justify-between text-[10px] text-muted-foreground">
        <span>{label}</span>
        <Icon className="size-3.5" />
      </div>
      <p className="mt-2 font-mono text-xl font-medium tracking-tight">
        {value}
      </p>
    </div>
  );
}
function Instrument({
  icon: Icon,
  label,
  value,
}: {
  icon: MetricIcon;
  label: string;
  value: string;
}) {
  return (
    <div>
      <p className="flex items-center gap-1 text-[10px] text-muted-foreground">
        <Icon className="size-3" />
        {label}
      </p>
      <p className="mt-1.5 font-mono text-sm font-semibold">{value}</p>
    </div>
  );
}
