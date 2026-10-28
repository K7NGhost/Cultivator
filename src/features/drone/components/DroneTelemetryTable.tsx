import { ChevronLeft, ChevronRight, LoaderCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { TelemetryPage, FlightRecord } from "../types";

export type TelemetryTableProps = {
  table: TelemetryPage | null;
  selected: FlightRecord;
  onSelect: (record: FlightRecord) => void;
  query: string;
  onQuery: (query: string) => void;
  page: number;
  onPage: (page: number) => void;
  paging: boolean;
};

export function DroneTelemetryTable({
  table,
  selected,
  onSelect,
  query,
  onQuery,
  page,
  onPage,
  paging,
}: TelemetryTableProps) {
  const size = table?.pageSize ?? 100;
  return (
    <section
      className="flex h-full min-h-0 flex-col"
      aria-label="Decoded telemetry"
    >
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <span className="text-muted-foreground">
          {table?.total.toLocaleString() ?? 0} records
        </span>
        {paging && <LoaderCircle className="size-3 animate-spin" />}
        <Input
          aria-label="Search telemetry"
          value={query}
          onChange={(e) => onQuery(e.target.value)}
          placeholder="Search decoded values…"
          className="ml-auto h-7 w-56 rounded-sm text-xs"
        />
      </div>
      <Table
        className="whitespace-nowrap font-mono text-[10px]"
        containerClassName="min-h-0 flex-1 overflow-auto"
      >
        <TableHeader className="sticky top-0 z-10 bg-muted">
          <TableRow>
            {table?.columns.map((column) => (
              <TableHead key={column} className="h-8 px-2 text-[10px]">
                {column}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {table?.rows.map((row) => (
            <TableRow
              key={row.telemetry.id}
              onClick={() => onSelect(row.telemetry)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSelect(row.telemetry);
                }
              }}
              tabIndex={0}
              aria-selected={row.telemetry.id === selected.id}
              data-state={
                row.telemetry.id === selected.id ? "selected" : undefined
              }
              className="cursor-pointer"
            >
              {row.values.map((value, i) => (
                <TableCell key={i} className="px-2 py-1">
                  {value || <span className="text-muted-foreground">—</span>}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
      {!table?.rows.length && (
        <p className="p-2 text-muted-foreground">
          No matching telemetry records.
        </p>
      )}
      <div className="flex h-8 shrink-0 items-center justify-between border-t px-2 text-muted-foreground">
        <span>
          Page {page + 1} of{" "}
          {Math.max(1, Math.ceil((table?.total ?? 0) / size))}
        </span>
        <div className="flex gap-1">
          <Button
            size="icon"
            variant="ghost"
            className="size-6"
            disabled={page === 0 || paging}
            aria-label="Previous telemetry page"
            onClick={() => onPage(page - 1)}
          >
            <ChevronLeft className="size-3.5" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-6"
            disabled={(page + 1) * size >= (table?.total ?? 0) || paging}
            aria-label="Next telemetry page"
            onClick={() => onPage(page + 1)}
          >
            <ChevronRight className="size-3.5" />
          </Button>
        </div>
      </div>
    </section>
  );
}
