import { useCallback, useEffect, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDownToLine, LoaderCircle, Plug, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useCases } from "@/features/cases";
import type { CaseRecord } from "@/features/cases/model/types";
import { FlightWorkspace } from "../components/FlightWorkspace";
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
      <div className="flex items-center gap-2 border-b p-2 text-xs text-muted-foreground">
        Open a case to inspect flight logs.
        <Button asChild variant="outline" size="sm" className="h-7 text-xs">
          <Link to="/case">Open case</Link>
        </Button>
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
  const flight = flights.find((item) => item.id === flightId);
  const refresh = useCallback(async (showLoading = true) => {
    if (showLoading) {
      setLoading(true);
      setError(null);
    }
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
      if (showLoading) setLoading(false);
    }
  }, [caseRecord.databasePath]);
  useEffect(() => {
    void refresh();
    const listener = listen("plugin-run-finished", () => {
      void refresh();
    });
    // Flights can finish before other plugins in the same run. Show their
    // persisted results without waiting for that entire run to finish.
    const timer = window.setInterval(() => {
      if (active) void refresh(false);
    }, 5000);
    return () => {
      window.clearInterval(timer);
      void listener.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [refresh, active]);
  useEffect(() => {
    let disposed = false;
    setDetail(null);
    select(null);
    setTable(null);
    setPage(0);
    setQuery("");
    setFilter("");
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
      className="flex h-full min-h-0 min-w-0 flex-col bg-background text-xs"
    >
      <div
        className="flex h-10 shrink-0 items-center gap-1 border-b px-2"
        aria-label="Flight actions"
      >
        <Button
          size="icon"
          variant="ghost"
          className="size-7"
          onClick={() => void refresh()}
          disabled={loading}
          aria-label="Refresh flights"
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />
        </Button>
        <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" asChild>
          <Link to="/plugins">
            <Plug className="size-3.5" />
            Run plugin
          </Link>
        </Button>
        <Button
          size="sm"
          variant="ghost"
          className="h-7 px-2 text-xs"
          disabled={!detail}
          onClick={() => void exportCsv()}
        >
          <ArrowDownToLine className="size-3.5" />
          Export CSV
        </Button>
        {flights.length > 0 && (
          <select
            aria-label="Select flight"
            className="select select-xs ml-auto w-64 rounded-sm border-border bg-background"
            value={flightId}
            onChange={(e) => setFlightId(e.target.value)}
          >
            {flights.map((item) => (
              <option key={item.id} value={item.id}>
                {item.label}
              </option>
            ))}
          </select>
        )}
      </div>
      {error && (
        <div
          role="alert"
          className="alert rounded-none border-0 border-b bg-destructive/10 px-2 py-1 text-xs text-destructive"
        >
          {error}
        </div>
      )}
      {!flights.length ? (
        <div className="flex items-center gap-2 p-2 text-muted-foreground">
          {loading ? (
            <>
              <LoaderCircle className="size-3.5 animate-spin" />
              Loading flights…
            </>
          ) : (
            <>
              No decoded flights. Add a datasource containing DAT logs and run
              Drone Telemetry from Plugins.
            </>
          )}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1">
          <aside
            className="hidden w-52 shrink-0 flex-col border-r lg:flex"
            aria-label="Flight library"
          >
            <div className="flex h-8 shrink-0 items-center justify-between border-b px-2 font-medium text-muted-foreground">
              <span>Flight logs</span>
              <span>{flights.length}</span>
            </div>
            <div className="min-h-0 flex-1 overflow-auto py-1">
              {flights.map((item) => (
                <Button
                  key={item.id}
                  variant="ghost"
                  onClick={() => setFlightId(item.id)}
                  aria-pressed={item.id === flightId}
                  className={`h-auto w-full justify-start rounded-none px-2 py-2 text-left text-xs ${item.id === flightId ? "bg-accent text-accent-foreground" : ""}`}
                >
                  <span className="min-w-0">
                    <span className="block truncate" title={item.label}>
                      {item.label}
                    </span>
                    <span className="block truncate text-[10px] font-normal text-muted-foreground">
                      {item.payload.metadata.aircraft}
                    </span>
                  </span>
                </Button>
              ))}
            </div>
          </aside>
          <main className="flex min-h-0 min-w-0 flex-1 flex-col">
            {opening ? (
              <div
                role="status"
                className="flex items-center gap-2 p-2 text-muted-foreground"
              >
                <LoaderCircle className="size-3.5 animate-spin" />
                Verifying and opening flight…
              </div>
            ) : detail &&
              selected &&
              flight &&
              detail.metadata.id === flight.payload.metadata.id ? (
              <FlightWorkspace
                key={flightId}
                active={active}
                detail={detail}
                selected={selected}
                onSelect={select}
                table={table}
                query={query}
                onQuery={setQuery}
                page={page}
                onPage={setPage}
                paging={paging}
              />
            ) : (
              !error && (
                <p className="p-2 text-muted-foreground">
                  Select a flight to inspect its telemetry.
                </p>
              )
            )}
          </main>
        </div>
      )}
    </div>
  );
}
