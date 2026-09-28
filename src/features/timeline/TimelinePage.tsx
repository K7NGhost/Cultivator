import { useEffect, useMemo, useState } from "react";
import { useLocation } from "react-router-dom";
import { FixedSizeList, type ListChildComponentProps } from "react-window";
import { Clock3, Film, Image as ImageIcon, MessageSquare, Phone, RefreshCw, X } from "lucide-react";
import { toast } from "react-toastify";

import { useReveal } from "@/app/reveal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { listArtifacts } from "@/features/artifacts/artifactRepository";
import { indexAssistantData } from "@/features/assistant/assistantEngine";
import { useCases } from "@/features/cases/case-provider";
import { listMediaGallery } from "@/features/media/mediaRepository";
import {
  EVENT_TYPES,
  bucketEvents,
  buildTimelineEvents,
  eventsByHour,
  type TimelineEvent,
  type TimelineEventType,
} from "@/features/timeline/timelineEvents";
import { useElementSize } from "@/lib/useElementSize";
import { cn } from "@/lib/utils";

const TYPE_STYLE: Record<TimelineEventType, { label: string; icon: typeof Clock3; fill: string; text: string }> = {
  message: { label: "Messages", icon: MessageSquare, fill: "fill-sky-500", text: "text-sky-600" },
  call: { label: "Calls", icon: Phone, fill: "fill-emerald-500", text: "text-emerald-600" },
  photo: { label: "Photos", icon: ImageIcon, fill: "fill-amber-500", text: "text-amber-600" },
  video: { label: "Videos", icon: Film, fill: "fill-violet-500", text: "text-violet-600" },
};
const ROW_HEIGHT = 28;

/** Everything dated in the case on one filterable timeline. */
export function TimelinePage() {
  const { activeCase } = useCases();
  const location = useLocation();
  const isActive = location.pathname === "/timeline";
  const reveal = useReveal();
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [types, setTypes] = useState<Set<TimelineEventType>>(() => new Set(EVENT_TYPES));
  const [text, setText] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [listRef, listSize] = useElementSize<HTMLDivElement>();

  useEffect(() => {
    if (!activeCase || !isActive) {
      if (!activeCase) {
        setEvents([]);
      }
      return;
    }
    let isCurrent = true;
    setIsLoading(true);
    Promise.all([listArtifacts(activeCase.databasePath), listMediaGallery(activeCase.databasePath, null)])
      .then(([artifacts, gallery]) => {
        if (isCurrent) {
          setEvents(buildTimelineEvents(indexAssistantData(artifacts, [...gallery.photos, ...gallery.videos])));
        }
      })
      .catch((error) => toast.error(error instanceof Error ? error.message : String(error)))
      .finally(() => {
        if (isCurrent) {
          setIsLoading(false);
        }
      });
    return () => {
      isCurrent = false;
    };
  }, [activeCase, isActive, refreshKey]);

  const filtered = useMemo(() => {
    const needle = text.trim().toLowerCase();
    const start = from ? new Date(`${from}T00:00:00`) : null;
    const end = to ? new Date(`${to}T23:59:59.999`) : null;
    return events.filter(
      (event) =>
        types.has(event.type) &&
        (!start || event.at >= start) &&
        (!end || event.at <= end) &&
        (!needle || `${event.title} ${event.detail}`.toLowerCase().includes(needle)),
    );
  }, [events, types, text, from, to]);
  const { unit, buckets } = useMemo(() => bucketEvents(filtered), [filtered]);
  const hours = useMemo(() => eventsByHour(filtered), [filtered]);
  const counts = useMemo(() => {
    const byType = { message: 0, call: 0, photo: 0, video: 0 };
    for (const event of events) byType[event.type] += 1;
    return byType;
  }, [events]);
  const peak = Math.max(1, ...buckets.map((bucket) => bucket.total));
  const peakHour = Math.max(1, ...hours);
  const hasFilter = Boolean(text || from || to) || types.size < EVENT_TYPES.length;

  function zoomTo(start: Date, end: Date) {
    const last = new Date(end.getTime() - 1);
    setFrom(dateInputValue(start));
    setTo(dateInputValue(last));
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <Clock3 className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <h1 className="text-sm font-semibold">Timeline</h1>
        <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
          {filtered.length.toLocaleString()} of {events.length.toLocaleString()} events
        </Badge>
        <div className="flex items-center gap-1">
          {EVENT_TYPES.map((type) => {
            const { label, icon: Icon, text: color } = TYPE_STYLE[type];
            const isOn = types.has(type);
            return (
              <button
                key={type}
                type="button"
                aria-pressed={isOn}
                className={cn(
                  "flex h-6 items-center gap-1 rounded-sm border px-1.5 text-[11px]",
                  isOn ? "bg-accent" : "text-muted-foreground opacity-60",
                )}
                onClick={() =>
                  setTypes((current) => {
                    const next = new Set(current);
                    if (isOn) next.delete(type);
                    else next.add(type);
                    return next;
                  })
                }
              >
                <Icon className={cn("size-3", color)} aria-hidden="true" />
                {label} {counts[type].toLocaleString()}
              </button>
            );
          })}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="ml-auto h-7 rounded-sm px-2 text-xs"
          disabled={!activeCase || isLoading}
          onClick={() => setRefreshKey((key) => key + 1)}
        >
          <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
          Refresh
        </Button>
      </section>

      <section className="flex shrink-0 flex-wrap items-center gap-2 border-b px-2 py-1.5 text-xs">
        <Input
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder="Filter by person or text"
          className="h-7 w-56 rounded-sm text-xs"
          aria-label="Filter timeline"
        />
        <label className="flex items-center gap-1 text-muted-foreground">
          From
          <input type="date" value={from} onChange={(event) => setFrom(event.target.value)} className="h-7 rounded-sm border bg-background px-1 text-xs" />
        </label>
        <label className="flex items-center gap-1 text-muted-foreground">
          To
          <input type="date" value={to} onChange={(event) => setTo(event.target.value)} className="h-7 rounded-sm border bg-background px-1 text-xs" />
        </label>
        {hasFilter && (
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            onClick={() => {
              setText("");
              setFrom("");
              setTo("");
              setTypes(new Set(EVENT_TYPES));
            }}
          >
            <X className="size-3.5" />
            Clear filters
          </Button>
        )}
      </section>

      <section className="grid shrink-0 grid-cols-[minmax(0,1fr)_14rem] gap-3 border-b px-2 py-2">
        <div>
          <div className="mb-1 text-[11px] text-muted-foreground">
            Activity per {unit} &middot; click a bar to zoom in
          </div>
          <svg viewBox={`0 0 ${Math.max(buckets.length, 1)} 100`} preserveAspectRatio="none" className="h-20 w-full" role="img" aria-label="Activity over time">
            {buckets.map((bucket, index) => {
              let y = 100;
              return (
                <g key={bucket.start.getTime()} className="cursor-pointer" onClick={() => zoomTo(bucket.start, bucket.end)}>
                  <rect x={index} y={0} width={1} height={100} fill="transparent" />
                  {EVENT_TYPES.map((type) => {
                    const height = (bucket.counts[type] / peak) * 100;
                    y -= height;
                    return height > 0 ? (
                      <rect key={type} x={index + 0.1} y={y} width={0.8} height={height} className={TYPE_STYLE[type].fill} />
                    ) : null;
                  })}
                  <title>{`${formatBucket(bucket.start, unit)}: ${bucket.total.toLocaleString()} events`}</title>
                </g>
              );
            })}
          </svg>
          {buckets.length > 0 && (
            <div className="flex justify-between text-[10px] text-muted-foreground">
              <span>{formatBucket(buckets[0].start, unit)}</span>
              <span>{formatBucket(buckets[buckets.length - 1].start, unit)}</span>
            </div>
          )}
        </div>
        <div>
          <div className="mb-1 text-[11px] text-muted-foreground">Activity by hour of day</div>
          <svg viewBox="0 0 24 100" preserveAspectRatio="none" className="h-20 w-full" role="img" aria-label="Activity by hour of day">
            {hours.map((count, hour) => (
              <rect key={hour} x={hour + 0.1} y={100 - (count / peakHour) * 100} width={0.8} height={(count / peakHour) * 100} className="fill-slate-500">
                <title>{`${String(hour).padStart(2, "0")}:00 – ${count.toLocaleString()} events`}</title>
              </rect>
            ))}
          </svg>
          <div className="flex justify-between text-[10px] text-muted-foreground">
            <span>00</span>
            <span>12</span>
            <span>23</span>
          </div>
        </div>
      </section>

      <div ref={listRef} className="min-h-0 flex-1">
        {filtered.length === 0 ? (
          <p className="p-3 text-xs text-muted-foreground">
            {!activeCase
              ? "Open a case to see its timeline."
              : isLoading
                ? "Loading events..."
                : events.length === 0
                  ? "No dated events yet. Run a message parser and Image Metadata on a data source."
                  : "No events match these filters."}
          </p>
        ) : (
          listSize.height > 0 && (
            <FixedSizeList
              height={listSize.height}
              width={listSize.width}
              itemCount={filtered.length}
              itemSize={ROW_HEIGHT}
              itemData={{ events: filtered, onOpen: (event: TimelineEvent) => reveal(event.reveal) }}
            >
              {EventRow}
            </FixedSizeList>
          )
        )}
      </div>
    </div>
  );
}

function EventRow({
  index,
  style,
  data,
}: ListChildComponentProps<{ events: TimelineEvent[]; onOpen: (event: TimelineEvent) => void }>) {
  const event = data.events[index];
  const { icon: Icon, text: color } = TYPE_STYLE[event.type];
  return (
    <button
      type="button"
      style={style}
      className="flex w-full items-center gap-2 border-b px-2 text-left text-xs hover:bg-accent"
      onClick={() => data.onOpen(event)}
      title="Open"
    >
      <span className="w-32 shrink-0 font-mono text-[11px] text-muted-foreground">
        {event.at.toLocaleString(undefined, { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}
      </span>
      <Icon className={cn("size-3.5 shrink-0", color)} aria-hidden="true" />
      <span className="w-56 shrink-0 truncate font-medium">{event.title}</span>
      <span className="min-w-0 truncate text-muted-foreground">{event.detail}</span>
    </button>
  );
}

function dateInputValue(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function formatBucket(date: Date, unit: "day" | "month") {
  return unit === "day"
    ? date.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" })
    : date.toLocaleDateString(undefined, { year: "numeric", month: "short" });
}
