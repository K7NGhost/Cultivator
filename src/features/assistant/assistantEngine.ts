import type { StoredArtifactRecord } from "@/features/artifacts/types";
import type { MediaItem } from "@/features/media/types";

import { inYearlessWindow } from "./assistantDateRange";
import {
  buildPersonDirectory,
  digitsOf,
  splitParticipant,
  type AssistantDateRange,
  type AssistantPerson,
  type AssistantQuery,
  type AssistantTarget,
} from "./assistantQuery";

export type AssistantMessage = {
  id: string;
  sender: string;
  recipients: string[];
  body: string;
  at: Date | null;
  service: string;
  filePath: string;
};

export type AssistantCall = {
  id: string;
  direction: string;
  contactName: string;
  phone: string;
  at: Date | null;
  durationSeconds: number | null;
  filePath: string;
};

export type AssistantContact = {
  id: string;
  name: string;
  phones: string[];
  emails: string[];
  notes: string;
  filePath: string;
};

export type AssistantMedia = {
  item: MediaItem;
  at: Date | null;
};

export type AssistantIndex = {
  messages: AssistantMessage[];
  calls: AssistantCall[];
  contacts: AssistantContact[];
  photos: AssistantMedia[];
  videos: AssistantMedia[];
  people: AssistantPerson[];
};

/** Why a result matched the content part of a question. */
export type AssistantMatch = {
  reason: "exact" | "meaning";
  tier: "strong" | "related";
  score?: number;
  /** A meaning match confirmed by the precision check. */
  checked?: boolean;
};

export type Matched<T> = T & { match?: AssistantMatch };

/**
 * Meaning-search scores from the local model, keyed by `semanticKey(...)`.
 * Only items that cleared the related cutoff are present.
 */
export type SemanticScores = Map<
  string,
  { score: number; tier: "strong" | "related"; checked: boolean }
>;

export type AssistantResults = {
  searched: AssistantTarget[];
  messages: Matched<AssistantMessage>[];
  calls: AssistantCall[];
  contacts: AssistantContact[];
  photos: Matched<AssistantMedia>[];
  videos: Matched<AssistantMedia>[];
  notes: string[];
};

export function semanticKey(refKind: "artifact" | "media", refId: string) {
  return `${refKind}:${refId}`;
}

export const ALL_TARGETS: AssistantTarget[] = ["messages", "calls", "contacts", "photos", "videos"];

const MESSAGE_KINDS = new Set(["message", "sms", "mms", "instant_message"]);

/** Normalizes stored plugin artifacts and gallery media into searchable records. */
export function indexAssistantData(
  artifacts: StoredArtifactRecord[],
  media: MediaItem[],
): AssistantIndex {
  const messages: AssistantMessage[] = [];
  const calls: AssistantCall[] = [];
  const contacts: AssistantContact[] = [];

  for (const record of artifacts) {
    const payload = readPayload(record.payload);
    const kind = String(payload.kind ?? record.resultKind ?? "");

    if (MESSAGE_KINDS.has(kind)) {
      messages.push({
        id: record.id,
        sender: String(payload.sender ?? payload.from ?? ""),
        recipients: toStringList(payload.recipients ?? payload.to),
        body: String(payload.body ?? ""),
        at: parseDate(payload.sentAt ?? payload.receivedAt ?? firstTimestamp(payload)),
        service: String(payload.service ?? ""),
        filePath: record.filePath,
      });
    } else if (kind === "call") {
      calls.push({
        id: record.id,
        direction: String(payload.direction ?? "unknown"),
        contactName: String(payload.contactName ?? ""),
        phone: String(payload.phone ?? ""),
        at: parseDate(payload.startedAt ?? firstTimestamp(payload)),
        durationSeconds: typeof payload.durationSeconds === "number" ? payload.durationSeconds : null,
        filePath: record.filePath,
      });
    } else if (kind === "contact") {
      contacts.push({
        id: record.id,
        name: String(payload.name ?? payload.label ?? record.label ?? ""),
        phones: toStringList(payload.phones),
        emails: toStringList(payload.emails),
        notes: String(payload.notes ?? ""),
        filePath: record.filePath,
      });
    }
  }

  const photos: AssistantMedia[] = [];
  const videos: AssistantMedia[] = [];
  for (const item of media) {
    const entry = { item, at: mediaDate(item) };
    (item.mediaType === "video" ? videos : photos).push(entry);
  }

  const people = buildPersonDirectory([
    ...contacts.map((contact) => ({ name: contact.name, phones: contact.phones })),
    ...messages.flatMap((message) =>
      [message.sender, ...message.recipients].map((value) => {
        const { name, phone } = splitParticipant(value);
        return { name, phones: [phone] };
      }),
    ),
    ...calls.map((call) => ({ name: call.contactName, phones: [call.phone] })),
  ]);

  return { messages, calls, contacts, photos, videos, people };
}

/**
 * Filters the index by the parsed question. When `semantic` scores are
 * given, a message or photo also matches the question's content words by
 * meaning (not only by exact words), and results are ranked by relevance.
 */
export function runAssistantQuery(
  query: AssistantQuery,
  index: AssistantIndex,
  semantic?: SemanticScores | null,
): AssistantResults {
  const searched = query.targets.length > 0 ? query.targets : ALL_TARGETS;
  const notes = [...query.notes];
  const allPeople = [...query.people.with, ...query.people.from, ...query.people.to];
  const hasPeople = allPeople.length > 0;

  const results: AssistantResults = {
    searched,
    messages: [],
    calls: [],
    contacts: [],
    photos: [],
    videos: [],
    notes,
  };

  if (searched.includes("messages")) {
    results.messages = index.messages
      .flatMap((message) => {
        const participants = [message.sender, ...message.recipients];
        if (!query.people.with.every((person) => participants.some((value) => personMatches(person, value)))) {
          return [];
        }
        if (query.people.from.length > 0 && !query.people.from.some((person) => personMatches(person, message.sender))) {
          return [];
        }
        if (
          query.people.to.length > 0 &&
          !query.people.to.some((person) => message.recipients.some((value) => personMatches(person, value)))
        ) {
          return [];
        }
        if (!inDateRange(message.at, query.dateRange)) {
          return [];
        }
        const match = contentMatch(
          [message.body, ...participants].join(" "),
          semanticKey("artifact", message.id),
          query.keywords,
          semantic,
        );
        return match === null ? [] : [{ ...message, match }];
      })
      .sort(byRelevance);
  }

  if (searched.includes("calls")) {
    // A call record only names the other party; the device owner is implicit.
    // So a call matches when any requested person is that other party.
    results.calls = index.calls
      .filter((call) => {
        const party = `${call.contactName} ${call.phone}`;
        if (hasPeople && !allPeople.some((person) => personMatches(person, party))) {
          return false;
        }
        if (query.people.from.length > 0 && !["incoming", "missed"].includes(call.direction)) {
          return false;
        }
        if (query.people.to.length > 0 && call.direction !== "outgoing") {
          return false;
        }
        return (
          inDateRange(call.at, query.dateRange) &&
          matchesKeywords(`${call.direction} ${party}`, query.keywords)
        );
      })
      .sort(byDate);
  }

  if (searched.includes("contacts")) {
    results.contacts = index.contacts.filter((contact) => {
      const haystack = [contact.name, ...contact.phones, ...contact.emails, contact.notes].join(" ");
      if (hasPeople && !allPeople.some((person) => personMatches(person, haystack))) {
        return false;
      }
      return matchesKeywords(haystack, query.keywords);
    });
    if (query.dateRange && query.targets.includes("contacts")) {
      notes.push("Contacts have no dates, so the date filter was ignored for them.");
    }
  }

  const mediaTargeted = query.targets.some((target) => target === "photos" || target === "videos");
  const skipMedia = hasPeople && !mediaTargeted;

  if (skipMedia && (searched.includes("photos") || searched.includes("videos"))) {
    notes.push(
      'Photos and videos aren\'t linked to people, so they were left out. Ask for "photos …" to include them.',
    );
  } else {
    if (hasPeople && mediaTargeted) {
      notes.push("Photos and videos can't be filtered by person yet, so the people filter was ignored for them.");
    }

    for (const target of ["photos", "videos"] as const) {
      if (!searched.includes(target)) {
        continue;
      }
      results[target] = index[target]
        .flatMap((entry) => {
          if (!inDateRange(entry.at, query.dateRange)) {
            return [];
          }
          const match = contentMatch(
            [entry.item.name, entry.item.caption ?? "", ...(entry.item.tags ?? [])].join(" "),
            semanticKey("media", entry.item.path),
            query.keywords,
            semantic,
          );
          return match === null ? [] : [{ ...entry, match }];
        })
        .sort(byRelevance);
    }

    const captioned = index.photos.some((entry) => entry.item.caption);
    if (query.keywords.length > 0 && searched.includes("photos") && index.photos.length > 0 && !captioned) {
      notes.push(
        "No photos have AI captions yet, so photo content can't be searched. Run the Media Tagging (AI) plugin.",
      );
    }
  }

  return results;
}

/**
 * Every message exchanged among exactly the same people as `message`, oldest
 * first, so a single hit can be read in the context of its conversation.
 */
export function conversationOf(message: AssistantMessage, messages: AssistantMessage[]) {
  const key = conversationKey(message);
  return messages.filter((candidate) => conversationKey(candidate) === key).sort(byDate);
}

function conversationKey(message: AssistantMessage) {
  const participants = [message.sender, ...message.recipients].map((value) => {
    const { name, phone } = splitParticipant(value);
    const digits = digitsOf(phone);
    return digits.length >= 7 ? digits : name.toLowerCase();
  });
  return Array.from(new Set(participants)).sort().join("|");
}

export function personMatches(person: AssistantPerson, value: string) {
  const valueDigits = digitsOf(value);
  if (
    valueDigits.length >= 7 &&
    person.phones.some((phone) => valueDigits.includes(phone) || phone.endsWith(valueDigits))
  ) {
    return true;
  }

  const name = person.name.toLowerCase().trim();
  if (!name || digitsOf(name).length >= 7) {
    return false;
  }

  return new RegExp(`(?:^|[^a-z0-9])${escapeRegExp(name)}(?=$|[^a-z0-9])`).test(value.toLowerCase());
}

export function matchesKeywords(haystack: string, keywords: string[]) {
  const text = haystack.toLowerCase();

  return keywords.every((keyword) =>
    keywordVariants(keyword).some((variant) =>
      /^[a-z0-9]+$/.test(variant)
        ? new RegExp(`(?:^|[^a-z0-9])${escapeRegExp(variant)}`).test(text)
        : text.includes(variant),
    ),
  );
}

export function inDateRange(date: Date | null, range: AssistantDateRange | null) {
  if (!range) {
    return true;
  }
  if (!date) {
    return false;
  }
  if (range.start && date < range.start) {
    return false;
  }
  if (range.end && date > range.end) {
    return false;
  }
  return range.monthsOfYear ? inYearlessWindow(date, range.monthsOfYear) : true;
}

/** Parses ISO timestamps and EXIF-style "2025:07:04 20:10:00" local times. */
export function parseDate(value: unknown): Date | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  if (typeof value === "number") {
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? null : date;
  }

  const text = String(value).replace(/^"|"$/g, "").trim();
  const exif = text.match(/^(\d{4})[:-](\d{2})[:-](\d{2})[ T](\d{2}):(\d{2}):(\d{2})$/);
  if (exif) {
    return new Date(
      Number(exif[1]), Number(exif[2]) - 1, Number(exif[3]),
      Number(exif[4]), Number(exif[5]), Number(exif[6]),
    );
  }

  const date = new Date(text);
  return Number.isNaN(date.getTime()) ? null : date;
}

function mediaDate(item: MediaItem) {
  const taken = item.metadata.find((field) =>
    ["DateTimeOriginal", "DateTimeDigitized"].includes(field.label),
  );
  return parseDate(taken?.value) ?? (item.modifiedMs ? new Date(item.modifiedMs) : null);
}

function keywordVariants(keyword: string) {
  const variants = [keyword.toLowerCase()];
  if (keyword.length > 4 && keyword.endsWith("ies")) {
    variants.push(`${keyword.slice(0, -3)}y`);
  } else if (keyword.length > 3 && keyword.endsWith("es")) {
    variants.push(keyword.slice(0, -2), keyword.slice(0, -1));
  } else if (keyword.length > 3 && keyword.endsWith("s")) {
    variants.push(keyword.slice(0, -1));
  }
  return variants;
}

function readPayload(payload: StoredArtifactRecord["payload"]): Record<string, unknown> {
  if (payload && typeof payload === "object") {
    return payload as Record<string, unknown>;
  }
  if (typeof payload === "string") {
    try {
      const parsed = JSON.parse(payload);
      return parsed && typeof parsed === "object" ? parsed : {};
    } catch {
      return {};
    }
  }
  return {};
}

function firstTimestamp(payload: Record<string, unknown>) {
  const timestamps = payload.timestamps;
  if (Array.isArray(timestamps) && timestamps[0] && typeof timestamps[0] === "object") {
    return (timestamps[0] as { value?: unknown }).value;
  }
  return undefined;
}

function toStringList(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.map(String).filter(Boolean);
  }
  return value ? [String(value)] : [];
}

/**
 * `undefined`: the question has no content words, so everything passes.
 * `null`: content words given and the item matches neither exactly nor by meaning.
 */
function contentMatch(
  haystack: string,
  key: string,
  keywords: string[],
  semantic: SemanticScores | null | undefined,
): AssistantMatch | null | undefined {
  if (keywords.length === 0) {
    return undefined;
  }

  const scored = semantic?.get(key);
  if (matchesKeywords(haystack, keywords)) {
    return { reason: "exact", tier: "strong", score: scored?.score };
  }
  return scored
    ? { reason: "meaning", tier: scored.tier, score: scored.score, checked: scored.checked }
    : null;
}

function byDate<T extends { at: Date | null }>(left: T, right: T) {
  return (left.at?.getTime() ?? 0) - (right.at?.getTime() ?? 0);
}

/** Strong before related, exact before meaning-only, then best score, then oldest. */
function byRelevance<T extends { at: Date | null; match?: AssistantMatch }>(left: T, right: T) {
  const tier = (match?: AssistantMatch) => (match?.tier === "related" ? 1 : 0);
  const reason = (match?: AssistantMatch) => (match?.reason === "meaning" ? 1 : 0);
  const score = (match?: AssistantMatch) => match?.score ?? 0;

  return (
    tier(left.match) - tier(right.match) ||
    reason(left.match) - reason(right.match) ||
    score(right.match) - score(left.match) ||
    byDate(left, right)
  );
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
