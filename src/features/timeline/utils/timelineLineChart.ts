import type { ApexOptions } from "apexcharts";
import type { TimelineBucket } from "../services/timelineRepository";
import { formatTimelineBound, formatTimelineTick } from "./timelineGraph";
import { timelineSeries } from "./timelineSeries";

type LineChartOptions = {
  id: string;
  buckets: TimelineBucket[];
  from: number;
  to: number;
  dark: boolean;
  onZoom: (min: number, max: number) => void;
  onReset: () => void;
};

/** Adapted from daisyUI Charts 1.1.0, Line Chart One, for the compact timeline workspace. */
export function timelineLineChartOptions({ id, buckets, from, to, dark, onZoom, onReset }: LineChartOptions): ApexOptions {
  const points = timelineSeries(buckets, from, to);
  return {
    chart: {
      id, type: "area", height: 200, fontFamily: "inherit", foreColor: "var(--muted-foreground)",
      parentHeightOffset: 0, background: "transparent", animations: { enabled: false },
      toolbar: { show: true, autoSelected: "zoom", tools: { download: false, selection: false, zoom: true, zoomin: true, zoomout: true, pan: true, reset: true } },
      zoom: { enabled: true, type: "x", autoScaleYaxis: true, allowMouseWheelZoom: false },
      dropShadow: { enabled: false },
      events: {
        zoomed: (_chart, selection) => { if (selection) onZoom(selection.xaxis.min, selection.xaxis.max); },
        scrolled: (_chart, selection) => { if (selection) onZoom(selection.xaxis.min, selection.xaxis.max); },
        beforeResetZoom: () => { onReset(); },
      },
      accessibility: { enabled: true, description: "Event activity over time. Drag to zoom, or use the zoom controls.", keyboard: { enabled: true } },
    },
    theme: { mode: dark ? "dark" : "light" },
    stroke: { width: 2, curve: "smooth" },
    series: [{ name: "Events", data: points }],
    colors: ["var(--foreground)"],
    fill: {
      type: "gradient",
      gradient: {
        type: "vertical", shadeIntensity: 0,
        colorStops: [
          { offset: 0, color: "var(--foreground)", opacity: dark ? 0.3 : 0.22 },
          { offset: 100, color: "var(--foreground)", opacity: 0.025 },
        ],
      },
    },
    dataLabels: { enabled: false },
    markers: { size: buckets.length === 1 ? 4 : 0, hover: { size: 5 } },
    xaxis: {
      type: "datetime", min: from === to ? from - 1000 : from, max: from === to ? to + 1000 : to,
      labels: { datetimeUTC: true, style: { fontSize: "10px" }, formatter: (value, timestamp) => formatTimelineTick(timestamp ?? Number(value), to - from < 86_400_000) },
      axisBorder: { show: false }, axisTicks: { show: false }, tooltip: { enabled: false },
    },
    yaxis: { min: 0, decimalsInFloat: 0, labels: { style: { fontSize: "10px" }, formatter: (value) => Math.round(value).toLocaleString() } },
    grid: { show: true, borderColor: "var(--border)", strokeDashArray: 3, padding: { top: 0, right: 12, bottom: 0, left: 8 } },
    tooltip: {
      enabled: true, theme: dark ? "dark" : "light",
      custom: ({ dataPointIndex }: { dataPointIndex: number }) => {
        const bucket = points[dataPointIndex]?.bucket;
        if (!bucket) return "";
        // Only database-generated counts and formatted numeric dates enter this markup.
        return `<div style="padding:8px 10px;font-size:11px;color:var(--popover-foreground)"><strong>${bucket.count.toLocaleString()} events</strong><div>${formatTimelineBound(bucket.start)}</div><div>to ${formatTimelineBound(bucket.end)}</div></div>`;
      },
    },
    legend: { show: false },
    noData: { text: "No events in this range", style: { color: "var(--muted-foreground)", fontSize: "11px" } },
  };
}
