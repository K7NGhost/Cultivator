import type { KeyboardEvent } from "react";

import type { AssistantMatch, AssistantResults } from "@/features/assistant/assistantEngine";
import { splitParticipant, type AssistantTarget } from "@/features/assistant/assistantQuery";

export const MAX_ROWS = 200;

export const TARGET_LABELS: Record<AssistantTarget, string> = {
  messages: "Messages",
  calls: "Calls",
  contacts: "Contacts",
  photos: "Photos",
  videos: "Videos",
};

const DATE_TIME_FORMAT =new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

export function highlight(text: string, keywords: string[]) {
  const terms = keywords
    .flatMap((keyword) => [keyword, keyword.replace(/(ies|es|s)$/, "")])
    .filter((term) => term.length > 1)
    .sort((left, right) => right.length - left.length)
    .map((term) => term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));

  if (terms.length === 0) {
    return text;
  }

  const pattern = new RegExp(`(${terms.join("|")})`, "gi");
  return text.split(pattern).map((part, index) =>
    index % 2 === 1 ? (
      <mark key={index} className="rounded-sm bg-yellow-300/60 px-0.5 text-foreground dark:bg-yellow-500/40">
        {part}
      </mark>
    ) : (
      part
    ),
  );
}

export function participantName(value: string) {
  const { name, phone } = splitParticipant(value);
  return name || phone || value;
}

export function formatDate(date: Date | null) {
  return date ? DATE_TIME_FORMAT.format(date) : "Unknown date";
}

export function formatDuration(seconds: number | null) {
  if (seconds === null) {
    return "-";
  }
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

/** Calls and contacts never carry a match; only meaning hits can be "related". */
export function tierOf(item: object) {
  return (item as { match?: AssistantMatch }).match?.tier;
}

export function strongOnly<T extends object>(items: T[]) {
  return items.filter((item) => tierOf(item) !== "related");
}

export function relatedOnly<T extends object>(items: T[]) {
  return items.filter((item) => tierOf(item) === "related");
}

export function summarize(results: AssistantResults) {
  const parts = results.searched
    .map((target) => {
      const items: object[] = results[target];
      return { target, count: items.filter((item) => tierOf(item) !== "related").length };
    })
    .filter(({ count }) => count > 0)
    .map(({ target, count }) => `${count.toLocaleString()} ${TARGET_LABELS[target].toLowerCase()}`);

  return parts.length > 0 ? `Found ${parts.join(", ")}.` : "Nothing strongly matched these filters.";
}

/** Rows and tiles that open something when clicked or activated by keyboard. */
export function clickableProps(onActivate: () => void, label: string) {
  return {
    role: "button" as const,
    tabIndex: 0,
    "aria-label": label,
    onClick: onActivate,
    onKeyDown: (event: KeyboardEvent) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        onActivate();
      }
    },
  };
}
