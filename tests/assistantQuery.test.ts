import { describe, expect, test } from "bun:test";

import {
  conversationOf,
  indexAssistantData,
  parseDate,
  runAssistantQuery,
  semanticKey,
  type SemanticScores,
} from "../src/features/assistant/assistantEngine";
import { parseAssistantQuery } from "../src/features/assistant/assistantQuery";
import type { StoredArtifactRecord } from "../src/features/artifacts/types";
import type { MediaItem } from "../src/features/media/types";

const JOHN = "John Carter (+1-555-0142)";
const JANE = "Jane Miller (+1-555-0177)";
const MOM = "Mom (+1-555-0101)";

function artifact(id: string, payload: Record<string, unknown>): StoredArtifactRecord {
  return {
    id,
    jobId: "job-1",
    pluginId: "json-chat-export",
    datasourceId: "ds-1",
    filePath: "demo-phone/messages/sms_export.json",
    resultKind: String(payload.kind),
    label: id,
    payload: payload as StoredArtifactRecord["payload"],
    createdAt: "2026-09-25T00:00:00Z",
  };
}

function message(id: string, sender: string, recipient: string, sentAt: string, body: string) {
  return artifact(id, { kind: "message", sender, recipients: [recipient], sentAt, body });
}

function photo(name: string, taken: string, caption: string, tags: string[]): MediaItem {
  return {
    id: name,
    mediaType: "image",
    name,
    path: `demo-phone/DCIM/Camera/${name}`,
    format: "JPEG",
    size: 1000,
    mediaPath: `demo-phone/DCIM/Camera/${name}`,
    thumbnailPath: `demo-phone/DCIM/Camera/${name}`,
    metadata: [{ label: "DateTimeOriginal", value: taken }],
    caption,
    tags,
  };
}

const artifacts = [
  artifact("contact-john", { kind: "contact", name: "John Carter", phones: ["+1-555-0142"] }),
  artifact("contact-jane", { kind: "contact", name: "Jane Miller", phones: ["+1-555-0177"] }),
  artifact("contact-mom", { kind: "contact", name: "Mom", phones: ["+1-555-0101"] }),
  message("m-jan", JOHN, JANE, "2025-01-21T19:40:00Z", "Coffee sometime?"),
  message("m-mar", JANE, JOHN, "2025-03-07T16:34:00Z", "Perfect. Invoice it as pallet repairs."),
  message("m-apr", JOHN, JANE, "2025-04-02T18:26:00Z", "I'll handle Mike. Delete these texts."),
  message("m-may", JANE, JOHN, "2025-05-06T14:12:00Z", "I'll redo the invoices before the audit."),
  message("m-jun", JANE, JOHN, "2025-06-24T17:25:00Z", "Auditors didn't find anything."),
  message("m-jul", JOHN, JANE, "2025-07-18T23:51:00Z", "Did you see the news?"),
  message("m-mom", MOM, JOHN, "2025-04-28T20:13:00Z", "Happy birthday!"),
  message("m-spam", "+1-555-0000", JOHN, "2025-09-11T12:00:00Z", "Your package is on hold."),
  artifact("c-jane", { kind: "call", direction: "incoming", contactName: "Jane Miller", phone: "+1-555-0177", startedAt: "2025-02-27T22:55:00Z" }),
  artifact("c-mom", { kind: "call", direction: "outgoing", contactName: "Mom", phone: "+1-555-0101", startedAt: "2025-07-04T13:00:00Z" }),
];

const media = [
  photo("IMG_0331.jpg", "2025-03-21 10:05:00", "a red car parked on the street", ["red", "car", "parked", "street"]),
  photo("IMG_0305.jpg", "2025:02:21 19:45:00", "a pizza with pepperoni", ["pizza", "pepperoni"]),
  photo("IMG_0704.jpg", "2025:07:04 20:10:00", "a sunset over the ocean", ["sunset", "ocean"]),
  photo("IMG_0420.jpg", "2024:04:20 08:30:00", "a stop sign", ["stop", "sign"]),
];

const index = indexAssistantData(artifacts, media);
const now = new Date(2026, 8, 25, 12, 0, 0);

function ask(text: string) {
  const query = parseAssistantQuery(text, { people: index.people, now });
  return { query, results: runAssistantQuery(query, index) };
}

describe("assistant query parsing", () => {
  test("parses the example question into messages, two people, and a month range", () => {
    const { query } = ask("show me the messages between john and jane march - jun 2025");

    expect(query.targets).toEqual(["messages"]);
    expect(query.people.with.map((person) => person.name).sort()).toEqual(["Jane Miller", "John Carter"]);
    expect(query.dateRange?.start).toEqual(new Date(2025, 2, 1));
    expect(query.dateRange?.end).toEqual(new Date(2025, 5, 30, 23, 59, 59, 999));
    expect(query.keywords).toEqual([]);
  });

  test("keeps a person that sits between a month range and its year", () => {
    const { query } = ask("messages from march to june with jane in 2025");

    expect(query.people.with.map((person) => person.name)).toEqual(["Jane Miller"]);
    expect(query.dateRange?.start).toEqual(new Date(2025, 2, 1));
    expect(query.keywords).toEqual([]);
  });

  test("distinguishes sender and recipient roles", () => {
    const { query } = ask("texts from jane to john");

    expect(query.people.from.map((person) => person.name)).toEqual(["Jane Miller"]);
    expect(query.people.to.map((person) => person.name)).toEqual(["John Carter"]);
  });

  test("treats words after a content cue as keywords, not targets", () => {
    const { query } = ask("messages that mention photos");

    expect(query.targets).toEqual(["messages"]);
    expect(query.keywords).toEqual(["photos"]);
  });

  test("reads quoted phrases as exact keywords", () => {
    const { query } = ask('messages containing "delete these"');

    expect(query.keywords).toEqual(["delete these"]);
  });

  test("handles before/until modifiers", () => {
    const before = ask("messages before april 2025").query.dateRange;
    const until = ask("messages until april 2025").query.dateRange;

    expect(before?.start).toBeNull();
    expect(before?.end).toEqual(new Date(new Date(2025, 3, 1).getTime() - 1));
    expect(until?.end).toEqual(new Date(2025, 3, 30, 23, 59, 59, 999));
  });

  test("uses a year-agnostic window when no year is given", () => {
    const { query } = ask("pictures from april");

    expect(query.targets).toEqual(["photos"]);
    expect(query.dateRange?.monthsOfYear).toEqual({ from: 3, to: 3 });
  });

  test("does not read an ordinary 'may' as the month", () => {
    const { query } = ask("messages that may mention money");

    expect(query.dateRange).toBeNull();
    expect(query.keywords).toEqual(["money"]);
  });

  test("resolves a typed phone number to a known number", () => {
    const { query } = ask("texts from 555-0000");

    expect(query.people.from).toHaveLength(1);
    expect(query.people.from[0].phones).toContain("15550000");
  });
});

describe("date ranges without a year", () => {
  const window = (text: string) => ask(text).query.dateRange?.monthsOfYear;

  test("reads 'between April and March' in calendar order, not around the year", () => {
    expect(window("messages between april and march")).toEqual({ from: 2, to: 3 });
    expect(ask("messages between april and march").results.messages.map((result) => result.id)).toEqual([
      "m-mar",
      "m-apr",
      "m-mom",
    ]);
  });

  test("corrects a misspelled month and says so", () => {
    const { query } = ask("messages between apirl and march");

    expect(query.dateRange?.monthsOfYear).toEqual({ from: 2, to: 3 });
    expect(query.keywords).toEqual([]);
    expect(query.notes).toContain('Read "apirl" as April.');
    expect(window("messages from feburary to march")).toEqual({ from: 1, to: 2 });
  });

  test("never turns a name or place into a month", () => {
    expect(ask("messages from marc").query.dateRange).toBeNull();
    expect(ask("photos in augusta").query.dateRange).toBeNull();
  });

  test("keeps a real wrap over New Year but not one that covers the whole year", () => {
    expect(window("messages from november to february")).toEqual({ from: 10, to: 1 });

    const { query } = ask("messages from april to march");
    expect(query.dateRange?.monthsOfYear).toEqual({ from: 2, to: 3 });
    expect(query.notes).toHaveLength(1);
  });

  test("applies before, after, since and until to a month", () => {
    expect(window("messages before april")).toEqual({ from: 0, to: 2 });
    expect(window("messages until march")).toEqual({ from: 0, to: 2 });
    expect(window("messages after june")).toEqual({ from: 6, to: 11 });
    expect(window("messages since may")).toEqual({ from: 4, to: 11 });
    expect(ask("messages before april").results.messages.map((result) => result.id)).toEqual(["m-jan", "m-mar"]);
  });

  test("keeps day bounds", () => {
    const { query, results } = ask("messages between march 5 and april 10");

    expect(query.dateRange?.monthsOfYear).toEqual({ from: 2, fromDay: 5, to: 3, toDay: 10 });
    expect(query.dateRange?.label).toBe("Mar 5 – Apr 10 (any year)");
    expect(results.messages.map((result) => result.id)).toEqual(["m-mar", "m-apr"]);
    expect(window("messages on march 7")).toEqual({ from: 2, fromDay: 7, to: 2, toDay: 7 });
    expect(window("messages before march 5")).toEqual({ from: 0, to: 2, toDay: 4 });
  });

  test("swaps a range typed with years in the wrong order", () => {
    const range = ask("messages between june 2025 and march 2025").query.dateRange;

    expect(range?.start).toEqual(new Date(2025, 2, 1));
    expect(range?.end).toEqual(new Date(2025, 5, 30, 23, 59, 59, 999));
  });
});

describe("assistant query execution", () => {
  test("returns only John/Jane messages inside March - June 2025", () => {
    const { results } = ask("show me the messages between john and jane march - jun 2025");

    expect(results.messages.map((result) => result.id)).toEqual(["m-mar", "m-apr", "m-may", "m-jun"]);
  });

  test("filters by sender and keyword, matching word prefixes", () => {
    const { results } = ask("messages from jane that mention the audit");

    // "audit" also matches "Auditors" in the June message.
    expect(results.messages.map((result) => result.id)).toEqual(["m-may", "m-jun"]);
  });

  test("finds photos by caption with simple plural handling", () => {
    const { results } = ask("photos of cars");

    expect(results.photos.map((result) => result.item.name)).toEqual(["IMG_0331.jpg"]);
  });

  test("filters photos by EXIF date", () => {
    expect(ask("photos from july 2025").results.photos.map((result) => result.item.name)).toEqual(["IMG_0704.jpg"]);
    expect(ask("pictures from april").results.photos.map((result) => result.item.name)).toEqual(["IMG_0420.jpg"]);
  });

  test("matches calls on the other party only", () => {
    const { results } = ask("calls with jane");

    expect(results.calls.map((result) => result.id)).toEqual(["c-jane"]);
  });

  test("searches every type when none is named, leaving out media for people queries", () => {
    const { results } = ask("everything with jane in february 2025");

    expect(results.searched).toContain("photos");
    expect(results.calls.map((result) => result.id)).toEqual(["c-jane"]);
    expect(results.photos).toEqual([]);
    expect(results.notes.some((note) => note.includes("aren't linked to people"))).toBe(true);
  });
});

describe("meaning search", () => {
  function askWithMeaning(text: string, scores: SemanticScores) {
    const query = parseAssistantQuery(text, { people: index.people, now });
    return runAssistantQuery(query, index, scores);
  }

  test("adds messages that match by meaning but not by exact words", () => {
    const scores: SemanticScores = new Map([
      [semanticKey("artifact", "m-mar"), { score: 0.64, tier: "strong" as const }],
      [semanticKey("artifact", "m-apr"), { score: 0.55, tier: "related" as const }],
    ]);

    const results = askWithMeaning("messages about money", scores);

    expect(results.messages.map((message) => [message.id, message.match?.reason, message.match?.tier])).toEqual([
      ["m-mar", "meaning", "strong"],
      ["m-apr", "meaning", "related"],
    ]);
  });

  test("ranks exact hits before meaning-only hits and keeps structural filters", () => {
    const scores: SemanticScores = new Map([
      [semanticKey("artifact", "m-mar"), { score: 0.8, tier: "strong" as const }],
      [semanticKey("artifact", "m-jun"), { score: 0.7, tier: "strong" as const }],
      [semanticKey("artifact", "m-jul"), { score: 0.69, tier: "strong" as const }],
    ]);

    const results = askWithMeaning("messages from jane about the audit", scores);

    // m-may and m-jun ("Auditors") are exact hits; m-mar is meaning-only despite
    // its higher score; m-jul is from John, so the sender filter drops it.
    expect(results.messages.map((message) => [message.id, message.match?.reason])).toEqual([
      ["m-jun", "exact"],
      ["m-may", "exact"],
      ["m-mar", "meaning"],
    ]);
  });

  test("finds photos by meaning of their caption", () => {
    const scores: SemanticScores = new Map([
      [semanticKey("media", "demo-phone/DCIM/Camera/IMG_0331.jpg"), { score: 0.6, tier: "strong" as const }],
    ]);

    const results = askWithMeaning("photos of vehicles", scores);

    expect(results.photos.map((entry) => [entry.item.name, entry.match?.reason])).toEqual([
      ["IMG_0331.jpg", "meaning"],
    ]);
  });

  test("questions without content words ignore meaning scores", () => {
    const scores: SemanticScores = new Map([
      [semanticKey("artifact", "m-mom"), { score: 0.9, tier: "strong" as const }],
    ]);

    const results = askWithMeaning("messages between john and jane march - jun 2025", scores);

    expect(results.messages.map((message) => message.id)).toEqual(["m-mar", "m-apr", "m-may", "m-jun"]);
    expect(results.messages.every((message) => message.match === undefined)).toBe(true);
  });
});

describe("conversation threads", () => {
  test("collects every message between the same people, in both directions, oldest first", () => {
    const anchor = index.messages.find((message) => message.id === "m-may")!;

    expect(conversationOf(anchor, index.messages).map((message) => message.id)).toEqual([
      "m-jan",
      "m-mar",
      "m-apr",
      "m-may",
      "m-jun",
      "m-jul",
    ]);
  });

  test("keeps other conversations out", () => {
    const anchor = index.messages.find((message) => message.id === "m-mom")!;

    expect(conversationOf(anchor, index.messages).map((message) => message.id)).toEqual(["m-mom"]);
  });
});

describe("date parsing", () => {
  test("reads EXIF and ISO timestamps", () => {
    expect(parseDate("2025:07:04 20:10:00")).toEqual(new Date(2025, 6, 4, 20, 10, 0));
    expect(parseDate('"2025-07-04 20:10:00"')).toEqual(new Date(2025, 6, 4, 20, 10, 0));
    expect(parseDate("2025-03-07T16:34:00Z")?.toISOString()).toBe("2025-03-07T16:34:00.000Z");
    expect(parseDate("")).toBeNull();
  });
});
