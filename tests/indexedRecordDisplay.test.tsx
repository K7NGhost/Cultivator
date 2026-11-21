import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { indexedRecordDisplay } from "../src/features/search/utils/indexedRecordDisplay";
import { IndexedRecordPreview } from "../src/features/search/components/IndexedRecordPreview";
import type { IndexedHit } from "../src/features/search/services/indexedSearchRepository";

function hit(body: string, stage = "1", chunk = 0): IndexedHit {
  return { body, stage, chunk, source: "C:\\Evidence\\messages.db", group: `${stage}:artifact:2` };
}

describe("readable indexed records", () => {
  test("shows labeled fields and natural message summaries without JSON syntax", () => {
    const display = indexedRecordDisplay(hit('Messages\n{"message_sender":"Alice","receiver":"Bob","message":"Hello\\nMeet at 10?","from_me":false,"message_id":12345678901234567890}'));
    expect(display.title).toBe("Messages");
    expect(display.fields).toContainEqual({ label: "Message Sender", value: "Alice" });
    expect(display.fields).toContainEqual({ label: "Message", value: "Hello\nMeet at 10?" });
    expect(display.fields).toContainEqual({ label: "From Me", value: "No" });
    expect(display.fields).toContainEqual({ label: "Message ID", value: "12345678901234567890" });
    expect(display.summary).toStartWith("Message: Hello\nMeet at 10?");
    expect(display.summary).not.toContain('"message"');
    expect(display.partial).toBe(false);
  });

  test("renders nested objects, arrays, nulls and falsy values as readable fields", () => {
    const display = indexedRecordDisplay(hit('Artifact\n{"attachments":[{"file_name":"notes.pdf"}],"count":0,"deleted":true,"missing":null,"empty":"","recipients":[]}'));
    expect(display.fields).toContainEqual({ label: "Attachments › Item 1 › File Name", value: "notes.pdf" });
    expect(display.fields).toContainEqual({ label: "Count", value: "0" });
    expect(display.fields).toContainEqual({ label: "Deleted", value: "Yes" });
    expect(display.fields).toContainEqual({ label: "Missing", value: "No value recorded" });
    expect(display.fields).toContainEqual({ label: "Empty", value: "Empty" });
    expect(display.fields).toContainEqual({ label: "Recipients", value: "None" });
  });

  test("formats known filesystem sizes and timestamps without guessing artifact epochs", () => {
    const display = indexedRecordDisplay(hit('C:\\Evidence\\messages.db\n{"size":4096,"readonly":true,"modified":"1704067200000000000","times":{"created":{"seconds":1704067200,"nanos":0,"basis":"utc"}}}', "0"));
    expect(display.fields).toContainEqual({ label: "Size", value: "4 KiB (4,096 bytes)" });
    expect(display.fields).toContainEqual({ label: "Read only", value: "Yes" });
    expect(display.fields).toContainEqual({ label: "Modified", value: "2024-01-01 00:00:00 UTC" });
    expect(display.fields).toContainEqual({ label: "Times › Created", value: "2024-01-01 00:00:00 UTC" });
    const artifact = indexedRecordDisplay(hit('{"timestamp":1704067200,"id":1704067200000000000}'));
    expect(artifact.fields[0].value).toBe("1704067200");
    expect(artifact.fields[1].value).toBe("1704067200000000000");
  });

  test("recovers explicit fields from truncated and continuation chunks", () => {
    const first = indexedRecordDisplay(hit('Messages\n{"sender":"Alice","message":"Hello\\nlong message cut off'));
    expect(first.fields).toContainEqual({ label: "Sender", value: "Alice" });
    expect(first.fields).toContainEqual({ label: "Message", value: "Hello\nlong message cut off…" });
    expect(first.partial).toBe(true);
    const next = indexedRecordDisplay(hit('continued message text","receiver":"Bob","message_id":12345678901234567890}', "1", 1));
    expect(next.text).toBe("continued message text");
    expect(next.fields).toContainEqual({ label: "Receiver", value: "Bob" });
    expect(next.fields).toContainEqual({ label: "Message ID", value: "12345678901234567890" });
    expect(next.partial).toBe(true);
  });

  test("keeps plain file content exact and safely escapes evidence markup", () => {
    const source = hit(' {"message":"<script>alert(1)</script>"}\n', "2");
    const display = indexedRecordDisplay(source);
    expect(display.text).toBe(source.body);
    expect(display.fields).toEqual([]);
    const record = hit('Messages\n{"message":"<img src=x onerror=alert(1)>","constructor":"original"}');
    const html = renderToStaticMarkup(<IndexedRecordPreview hit={record} display={indexedRecordDisplay(record)} />);
    expect(html).toContain("Readable indexed fields");
    expect(html).toContain("&lt;img src=x onerror=alert(1)&gt;");
    expect(html).not.toContain("<img");
    expect(html).toContain("Constructor");
    expect(html).toContain("<details");
    expect(html).not.toContain("<details open");
  });

  test("malformed data cannot throw while displaying search results", () => {
    for (const body of ['{"count":001,"message":"cut', '{"bad\\q":"value","sender":"Alice"', 'fragment\\u123', '{', 'not JSON at all']) {
      expect(() => indexedRecordDisplay(hit(body))).not.toThrow();
    }
  });

  test("bounds large field lists and identifies excerpts instead of silently omitting data", () => {
    const record = Object.fromEntries(Array.from({ length: 250 }, (_, i) => [`f${i}`, i]));
    const display = indexedRecordDisplay(hit(JSON.stringify(record)));
    expect(display.fields).toHaveLength(200);
    expect(display.partial).toBe(true);
    const local = indexedRecordDisplay(hit('{"modified":{"seconds":1704067200,"nanos":0,"basis":"local_civil"}}', "0"), "America/New_York");
    expect(local.fields[0].value).toBe("2024-01-01 00:00:00.000 (recorded local time)");
  });
});
