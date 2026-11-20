import { useEffect, useId, useRef, useState } from "react";
import ApexCharts from "apexcharts";
import { ChartNoAxesCombined, ChevronDown, ChevronRight, LoaderCircle, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { TimelineOverview, TimelineRange } from "../services/timelineRepository";
import { formatTimelineBound, normalizeTimelineZoom } from "../utils/timelineGraph";
import { timelineLineChartOptions } from "../utils/timelineLineChart";
import "./timelineGraph.css";

type GraphProps = {
  overview: TimelineOverview | null;
  focusedOverview: TimelineOverview | null;
  range: TimelineRange | null;
  busy: boolean;
  focusing: boolean;
  error: string;
  onRangeChange: (range: TimelineRange | null) => void;
};

export function TimelineGraph(props: GraphProps) {
  const [expanded, setExpanded] = useState(true);
  const waitingForRange = props.range !== null && !props.focusedOverview;
  const loading = props.busy || props.focusing || (waitingForRange && !props.error);
  return <section className="shrink-0 border-b bg-muted/10" aria-label="Timeline graph" aria-busy={loading}>
    <div className="flex flex-wrap items-center gap-2 px-2 py-1">
      <Button variant="ghost" size="sm" className="h-7 gap-1 px-1 text-xs text-foreground" aria-expanded={expanded} aria-controls="timeline-graph-content" onClick={() => setExpanded(!expanded)}>
        {expanded ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        <ChartNoAxesCombined className="size-3.5" /> Event activity
      </Button>
      <span className="text-[11px] text-muted-foreground">All matching events · {props.busy ? "Loading…" : (props.overview?.total ?? 0).toLocaleString()} events</span>
      <Button variant="ghost" size="sm" className="ml-auto h-6 px-2 text-[11px] text-muted-foreground" disabled={!props.range || props.busy} onClick={() => props.onRangeChange(null)}>
        <RotateCcw className="size-3" /> Reset zoom
      </Button>
    </div>
    {expanded && <div id="timeline-graph-content">
      {loading ? <div role="status" className="grid place-items-center" style={{ height: 252 }}>
          <LoaderCircle className="size-6 animate-spin text-primary" aria-hidden="true" />
          <span className="sr-only">{props.range ? "Loading selected time range…" : "Loading event activity…"}</span>
        </div>
        : waitingForRange ? null
        : props.overview?.buckets.length ? <TimelineLinePlot {...props} overview={props.overview} />
        : <div role="status" className="grid h-24 place-items-center text-xs text-muted-foreground">No dated events match the filters.</div>}
      {props.error && <p role="alert" className="px-3 py-1 text-xs text-destructive">{props.error}</p>}
    </div>}
  </section>;
}

function TimelineLinePlot({ overview, focusedOverview, range, onRangeChange }: GraphProps & { overview: TimelineOverview }) {
  const host = useRef<HTMLDivElement>(null);
  const chartId = `timeline-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const callbacks = useRef({ onRangeChange, range });
  callbacks.current = { onRangeChange, range };
  const [renderError, setRenderError] = useState("");
  const [dark, setDark] = useState(() => document.documentElement.classList.contains("dark"));
  const buckets = range && focusedOverview ? focusedOverview.buckets : overview.buckets;
  const fullStart = overview.buckets[0].start;
  const fullEnd = overview.buckets[overview.buckets.length - 1].end;

  useEffect(() => {
    const observer = new MutationObserver(() => setDark(document.documentElement.classList.contains("dark")));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!host.current) return;
    let disposed = false;
    setRenderError("");
    const chart = new ApexCharts(host.current, timelineLineChartOptions({
      id: chartId, buckets, from: range?.from ?? fullStart, to: range?.to ?? fullEnd, dark,
      onZoom: (min, max) => {
        const next = normalizeTimelineZoom(min, max, fullStart, fullEnd);
        if (next === undefined) return;
        const current = callbacks.current.range;
        if (next?.from === current?.from && next?.to === current?.to) return;
        callbacks.current.onRangeChange(next);
      },
      onReset: () => callbacks.current.onRangeChange(null),
    }));
    void chart.render().catch((error: unknown) => {
      if (!disposed) setRenderError(error instanceof Error ? error.message : String(error));
    });
    return () => { disposed = true; chart.destroy(); };
  }, [buckets, range?.from, range?.to, fullStart, fullEnd, dark, chartId]);

  return <>
    <p className="px-3 text-[11px] text-muted-foreground">Drag across the line to zoom into a time range and show its events below. Drag again to zoom further, or use the chart controls.</p>
    <div ref={host} className="timeline-line-chart min-w-0 px-2" style={{ height: 200 }} aria-label="Event activity line graph with drag-to-zoom controls" />
    {renderError && <p role="alert" className="px-3 text-xs text-destructive">{renderError}</p>}
    <p className="px-3 pb-1 text-[11px] text-muted-foreground" role="status">
      {range
        ? `Zoomed range: ${formatTimelineBound(range.from)} — ${formatTimelineBound(range.to)}`
        : "Showing all matching events. Select a range to zoom in."}
    </p>
    <p className="px-3 pb-2 text-[10px] text-muted-foreground">Axis: UTC for known timezones; recorded clock for unspecified times. Date-only events use their recorded date. Each point counts events in an interval.</p>
  </>;
}
