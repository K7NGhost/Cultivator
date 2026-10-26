import { useMemo } from "react";
import { finiteLabel } from "../lib/telemetry";
import type { FlightRecord } from "../types";

export function TelemetryChart({
  records,
  selected,
  field,
  title,
  unit,
  color,
  onSelect,
}: {
  records: FlightRecord[];
  selected: FlightRecord;
  field: "altitude" | "speed";
  title: string;
  unit: string;
  color: string;
  onSelect: (record: FlightRecord) => void;
}) {
  const data = useMemo(() => {
    const stride = Math.max(1, Math.ceil(records.length / 600));
    const samples = records.filter(
      (_, index) => index % stride === 0 || index === records.length - 1,
    );
    const values = samples.flatMap((row) =>
      row[field] == null ? [] : [row[field]!],
    );
    const min = values.length ? Math.min(...values) : 0;
    const max = values.length ? Math.max(...values) : 1;
    const start = records[0]?.elapsedSeconds ?? 0;
    const duration = Math.max(
      0.001,
      (records[records.length - 1]?.elapsedSeconds ?? 0) - start,
    );
    let connected = false;
    const path = samples
      .map((row) => {
        const value = row[field];
        if (value == null || !Number.isFinite(value)) {
          connected = false;
          return "";
        }
        const x = 8 + (((row.elapsedSeconds ?? 0) - start) / duration) * 584;
        const y = 66 - ((value - min) / (max - min || 1)) * 52;
        const command = connected ? "L" : "M";
        connected = true;
        return `${command}${x.toFixed(2)},${y.toFixed(2)}`;
      })
      .join(" ");
    return { path, min, max, start, duration, hasValues: values.length > 0 };
  }, [records, field]);
  const cursor = Math.min(
    592,
    Math.max(
      8,
      8 + (((selected.elapsedSeconds ?? 0) - data.start) / data.duration) * 584,
    ),
  );
  return (
    <div className="border-r bg-background p-2">
      <div className="flex justify-between text-xs">
        <span className="text-muted-foreground">{title}</span>
        <span className="font-mono font-medium">
          {finiteLabel(selected[field])} {unit}
        </span>
      </div>
      {data.hasValues ? (
        <svg
          viewBox="0 0 600 80"
          className="mt-1 h-20 w-full cursor-crosshair"
          preserveAspectRatio="none"
          role="img"
          aria-label={`${title} over the GPS track. Click to select a point.`}
          onClick={(event) => {
            const box = event.currentTarget.getBoundingClientRect();
            const elapsed =
              data.start +
              Math.max(0, Math.min(1, (event.clientX - box.left) / box.width)) *
                data.duration;
            const row = records.reduce<FlightRecord | undefined>(
              (best, item) =>
                !best ||
                Math.abs((item.elapsedSeconds ?? 0) - elapsed) <
                  Math.abs((best.elapsedSeconds ?? 0) - elapsed)
                  ? item
                  : best,
              undefined,
            );
            if (row) onSelect(row);
          }}
        >
          <path
            d="M8 66H592 M8 40H592 M8 14H592"
            stroke="currentColor"
            opacity=".08"
          />
          <path
            d={data.path}
            fill="none"
            stroke={color}
            strokeWidth="2"
            vectorEffect="non-scaling-stroke"
          />
          <path
            d={`M${cursor} 8V72`}
            stroke="currentColor"
            opacity=".35"
            strokeDasharray="3 3"
          />
        </svg>
      ) : (
        <div className="grid h-20 place-items-center text-xs text-muted-foreground">
          No {title.toLowerCase()} on the GPS track
        </div>
      )}
      <div className="flex justify-between text-[9px] text-muted-foreground">
        <span>GPS track samples</span>
        <span>
          {finiteLabel(data.min)}–{finiteLabel(data.max)} {unit}
        </span>
      </div>
    </div>
  );
}
