import { Button } from "@/components/ui/button";
import type { MapDateFilter } from "../lib/dateFilter";
import { Link } from "react-router-dom";

export function DateRangeFilter({ value, onChange, error, timeZone }: {
  value: MapDateFilter; onChange: (value: MapDateFilter) => void; error: string | null; timeZone: string;
}) {
  const active = !!(value.from || value.to || value.fromTime || value.toTime);
  return <div className="shrink-0 border-b px-2 py-1" aria-label="Map date filter">
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
      <span className="font-medium">Date range</span>
      <label className="flex items-center gap-1">From <input type="date" className="input input-xs w-36 rounded-sm"
        aria-label="Map start date" value={value.from} max={value.to || undefined}
        onChange={(event) => onChange({ ...value, from: event.target.value })} /></label>
      <label className="flex items-center gap-1">To <input type="date" className="input input-xs w-36 rounded-sm"
        aria-label="Map end date" value={value.to} min={value.from || undefined}
        onChange={(event) => onChange({ ...value, to: event.target.value })} /></label>
      <span className="font-medium">Daily time range</span>
      <label className="flex items-center gap-1">From <input type="time" step="1" className="input input-xs w-32 rounded-sm"
        aria-label="Map start time" value={value.fromTime ?? ""} onChange={(event) => onChange({ ...value, fromTime: event.target.value })} /></label>
      <label className="flex items-center gap-1">To <input type="time" step="1" className="input input-xs w-32 rounded-sm"
        aria-label="Map end time" value={value.toTime ?? ""} onChange={(event) => onChange({ ...value, toTime: event.target.value })} /></label>
      <label className="flex items-center gap-1.5"><input type="checkbox" className="checkbox checkbox-xs" disabled={!active}
        checked={value.includeUndated} onChange={(event) => onChange({ ...value, includeUndated: event.target.checked })} />Include missing dates/times</label>
      <Button size="sm" variant="ghost" className="h-6 px-1.5 text-xs" disabled={!active}
        onClick={() => onChange({ from: "", to: "", fromTime: "", toTime: "", includeUndated: false })}>Clear filters</Button>
      <span className="text-[10px] text-muted-foreground">Timezone: <Link to="/settings" className="underline">{timeZone}</Link>. Both dates included. Time range repeats daily; an earlier end time crosses midnight. Unknown timezones keep recorded clocks.</span>
    </div>
    {error && <p role="alert" className="pt-1 text-destructive">{error}</p>}
  </div>;
}
