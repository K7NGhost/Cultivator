import { describe, expect, test } from "bun:test";

import {
  indexAssistantData,
  parseDate,
  runAssistantQuery,
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

describe("date parsing", () => {
  test("reads EXIF and ISO timestamps", () => {
    expect(parseDate("2025:07:04 20:10:00")).toEqual(new Date(2025, 6, 4, 20, 10, 0));
    expect(parseDate('"2025-07-04 20:10:00"')).toEqual(new Date(2025, 6, 4, 20, 10, 0));
    expect(parseDate("2025-03-07T16:34:00Z")?.toISOString()).toBe("2025-03-07T16:34:00.000Z");
    expect(parseDate("")).toBeNull();
  });
});
