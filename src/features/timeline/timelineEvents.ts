import type { RevealTarget } from "@/app/reveal";
import type { AssistantIndex } from "@/features/assistant/assistantEngine";

export const EVENT_TYPES = ["message", "call", "photo", "video"] as const;
export type TimelineEventType = (typeof EVENT_TYPES)[number];

export type TimelineEvent = {
  id: string;
  at: Date;
  type: TimelineEventType;
  title: string;
  detail: string;
  reveal: RevealTarget;
};

export type TimelineBucket = {
  start: Date;
  end: Date;
  counts: Record<TimelineEventType, number>;
  total: number;
};

/** Every dated message, call, photo and video in the case, oldest first. */
export function buildTimelineEvents(index: AssistantIndex): TimelineEvent[] {
  const events: TimelineEvent[] = [];

  for (const message of index.messages) {
    if (message.at) {
      events.push({
        id: `message:${message.id}`,
        at: message.at,
        type: "message",
        title: `${message.sender || "Unknown"} → ${message.recipients.join(", ") || "Unknown"}`,
        detail: message.body,
        reveal: { kind: "artifact", artifactId: message.id },
      });
    }
  }
  for (const call of index.calls) {
    if (call.at) {
      const duration =
        call.durationSeconds === null ? "" : ` · ${Math.round(call.durationSeconds / 60)} min`;
      events.push({
        id: `call:${call.id}`,
        at: call.at,
        type: "call",
        title: `${call.direction} call · ${call.contactName || call.phone || "Unknown"}`,
        detail: `${call.phone}${duration}`,
        reveal: { kind: "artifact", artifactId: call.id },
      });
    }
  }
  for (const [list, type] of [
    [index.photos, "photo"],
    [index.videos, "video"],
  ] as const) {
    for (const media of list) {
      if (media.at) {
        events.push({
          id: `${type}:${media.item.path}`,
          at: media.at,
          type,
          title: media.item.name,
          detail: media.item.caption ?? "",
          reveal: { kind: "media", path: media.item.path, mediaType: type === "video" ? "video" : "image" },
        });
      }
    }
  }

  return events.sort((left, right) => left.at.getTime() - right.at.getTime());
}

/**
 * Consecutive day buckets (month buckets past ~13 months) from the first
 * event to the last, empty ones included so gaps in activity show.
 */
export function bucketEvents(events: TimelineEvent[]): { unit: "day" | "month"; buckets: TimelineBucket[] } {
  if (events.length === 0) {
    return { unit: "day", buckets: [] };
  }

  let first = events[0].at;
  let last = events[0].at;
  for (const event of events) {
    if (event.at < first) first = event.at;
    if (event.at > last) last = event.at;
  }
  const unit = last.getTime() - first.getTime() > 400 * 86_400_000 ? "month" : "day";
  const startOf = (date: Date) =>
    unit === "day"
      ? new Date(date.getFullYear(), date.getMonth(), date.getDate())
      : new Date(date.getFullYear(), date.getMonth(), 1);
  const next = (date: Date) =>
    unit === "day"
      ? new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1)
      : new Date(date.getFullYear(), date.getMonth() + 1, 1);

  const buckets: TimelineBucket[] = [];
  const byStart = new Map<number, TimelineBucket>();
  for (let start = startOf(first); start <= last; start = next(start)) {
    const bucket = {
      start,
      end: next(start),
      counts: { message: 0, call: 0, photo: 0, video: 0 },
      total: 0,
    };
    buckets.push(bucket);
    byStart.set(start.getTime(), bucket);
  }
  for (const event of events) {
    const bucket = byStart.get(startOf(event.at).getTime());
    if (bucket) {
      bucket.counts[event.type] += 1;
      bucket.total += 1;
    }
  }
  return { unit, buckets };
}

/** Events per hour of the day (local time), for a pattern-of-life view. */
export function eventsByHour(events: TimelineEvent[]): number[] {
  const hours = Array.from({ length: 24 }, () => 0);
  for (const event of events) {
    hours[event.at.getHours()] += 1;
  }
  return hours;
}
