import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ArtifactMessagePreview } from "../src/features/artifacts/components/ArtifactMessagePreview";
import { artifactMessageSourceScopes, detectMessageColumns, formatMessageTime, getArtifactConversations } from "../src/features/artifacts/utils/artifactMessages";
import type { CustomTableColumn, StoredArtifactRecord } from "../src/features/artifacts/model/types";
import type { CustomTableView } from "../src/features/artifacts/utils/customTableGrouping";

function table(names: (string | CustomTableColumn)[], values: Record<string, unknown>[], sourceIndices: number[] = []): CustomTableView {
  return { name: "Messages", columns: names.map((column) => typeof column === "string" ? { key: column, label: column } : column),
    deduplication: { mode: "preserve" }, totalOccurrences: values.length,
    sources: [{ artifactId: "one", filePath: "/messages.db" }, { artifactId: "two", filePath: "/messages.db" }],
    rows: values.map((values, index) => ({ values, occurrenceCount: 1, primarySourceIndex: sourceIndices[index] ?? 0 })) };
}
function conversations(input: CustomTableView) {
  const columns = detectMessageColumns(input.columns);
  if (!columns) throw new Error("Message columns were not detected");
  return getArtifactConversations(input, columns);
}
function artifact(id: string, datasourceId: string): StoredArtifactRecord {
  return { id, datasourceId, pluginId: "parser", jobId: "job", resultKind: "message", label: "Message", filePath: "/messages.db", payload: {}, createdAt: "" };
}

describe("automatic message artifact previews", () => {
  test("supports the Message Sender and Other Participant/Conversation Name schema with attachment metadata", () => {
    const names = ["Message Timestamp", "Direction", "Message Sender", "Other Participant/Conversation Name", "Message", "Message Type", "Attachment Byte Size", "Attachment Location", "Conversation ID"];
    const input = table(names.map((label, index) => ({ key: `field_${index}`, label })), [
      { field_0: "2026-01-01T12:00:00Z", field_1: "Incoming", field_2: "Alice", field_3: "Project team", field_4: "Hello", field_5: "SMS", field_6: 0, field_7: "", field_8: 7 },
      { field_0: "2026-01-01T12:01:00Z", field_1: "Outgoing", field_2: "Kevin", field_3: "Project team", field_4: "", field_5: "MMS", field_6: 2048, field_7: "/attachments/notes.pdf", field_8: 7 },
    ]);
    const mapping = detectMessageColumns(input.columns)!;
    expect(mapping.sender?.key).toBe("field_2");
    expect(mapping.conversationName?.key).toBe("field_3");
    expect(mapping.recipients).toBeUndefined();
    expect(mapping.attachment?.key).toBe("field_7");
    const groups = getArtifactConversations(input, mapping);
    expect(groups).toHaveLength(1);
    expect(groups[0].title).toBe("Project team");
    expect(groups[0].messages.map((message) => message.direction)).toEqual(["incoming", "outgoing"]);
    expect(groups[0].messages[0].attachmentBytes).toBe("0");
    expect(groups[0].messages[1]).toMatchObject({ messageType: "MMS", attachmentBytes: "2048", attachment: "/attachments/notes.pdf" });
    const html = renderToStaticMarkup(<ArtifactMessagePreview table={input} columns={mapping} sourceArtifacts={[]} />);
    expect(html).toContain("Project team");
    expect(html).toContain("Attachment bytes: 2048");
    expect(html).toContain("MMS");
    expect(html).toContain("/attachments/notes.pdf");
    const snakeCase = table(names.map((name) => name.toLowerCase().replace(/[^a-z0-9]+/g, "_")), []);
    expect(detectMessageColumns(snakeCase.columns)?.sender?.key).toBe("message_sender");
  });

  test.each([
    ["sender", "recipients", "body"], ["Sender ID", "Receiver", "Message Text"],
    ["From", "To", "Content"], ["senderName", "recipientId", "messageBody"],
  ])("detects %s, %s and %s without a specialized artifact kind", (sender, recipient, body) => {
    const input = table([sender, recipient, body], []);
    const mapping = detectMessageColumns(input.columns);
    expect(mapping?.sender?.key).toBe(sender);
    expect(mapping?.recipients?.key).toBe(recipient);
    expect(mapping?.body.key).toBe(body);
  });

  test("uses human labels for generated column keys and avoids message IDs/status fields", () => {
    const cols = [{ key: "col1", label: "Sender" }, { key: "col2", label: "Message ID" }, { key: "col3", label: "Message Text" }, { key: "col4", label: "Message Timestamp (UTC)" }, { key: "col5", label: "Read Timestamp" }];
    expect(detectMessageColumns(cols)?.body.key).toBe("col3");
    expect(detectMessageColumns(cols)?.timestamp?.key).toBe("col4");
    expect(detectMessageColumns(cols.filter((column) => column.key !== "col3"))).toBeNull();
    expect(detectMessageColumns(table(["Author", "Body", "Title"], []).columns)).toBeNull();
    expect(detectMessageColumns(table(["Name", "Phone", "Email"], []).columns)).toBeNull();
    expect(detectMessageColumns(table(["Text", "From Me", "Chat ID"], []).columns)?.fromMe?.key).toBe("From Me");
  });

  test("keeps conversations, sources and datasources separate while allowing message records from the same source to combine", () => {
    const input = table(["sender", "recipients", "body", "conversationId"], [
      { sender: "Alice", recipients: ["Bob"], body: "A", conversationId: 7 },
      { sender: "Bob", recipients: ["Alice"], body: "B", conversationId: 7 },
      { sender: "Alice", recipients: ["Bob"], body: "Different thread", conversationId: 8 },
    ], [0, 1, 0]);
    const mapping = detectMessageColumns(input.columns)!;
    const sameSource = artifactMessageSourceScopes([artifact("one", "phone"), artifact("two", "phone")]);
    expect(getArtifactConversations(input, mapping, sameSource).map((thread) => thread.messages.length)).toEqual([2, 1]);
    const differentDevices = artifactMessageSourceScopes([artifact("one", "phone"), artifact("two", "other-phone")]);
    expect(getArtifactConversations(input, mapping, differentDevices)).toHaveLength(3);
    input.sources[1].filePath = "/different/messages.db";
    expect(getArtifactConversations(input, mapping)).toHaveLength(3);
  });

  test("groups bidirectional participant pairs, preserves group members and does not use message IDs as thread IDs", () => {
    const input = table(["Sender", "Receiver", "Message", "Message ID"], [
      { Sender: "Alice", Receiver: "Bob", Message: "One", "Message ID": 1 },
      { Sender: "Bob", Receiver: "Alice", Message: "Two", "Message ID": 2 },
      { Sender: "Alice", Receiver: "Charlie", Message: "Three", "Message ID": 3 },
    ]);
    const result = conversations(input);
    expect(result).toHaveLength(2);
    expect(result[0].basis).toBe("participants");
    expect(result[0].participants).toEqual(["Alice", "Bob"]);
    expect(result[0].messages.map((message) => message.rowIndex)).toEqual([0, 1]);
    const group = table(["Sender", "Message", "Chat ID", "Chat Name", "Chat Participants"], [
      { Sender: "Alice", Message: "Hi", "Chat ID": 0, "Chat Name": "Family", "Chat Participants": '["Alice","Bob","Charlie"]' },
      { Sender: "Charlie", Message: "Hello", "Chat ID": 0, "Chat Name": "Family", "Chat Participants": ["Charlie", "Bob", "Alice"] },
    ]);
    expect(conversations(group)[0]).toMatchObject({ title: "Family", thread: "0", participants: ["Alice", "Bob", "Charlie"] });
  });

  test("orders explicit timestamps, honors declared epochs and preserves raw numeric timestamps with unknown units", () => {
    const input = table(["sender", "body", { key: "sentAt", label: "Sent", format: "datetime" }], [
      { sender: "Alice", body: "Later", sentAt: "2026-01-01T12:00:00Z" },
      { sender: "Alice", body: "Earlier", sentAt: "2026-01-01T12:30:00+01:00" },
    ]);
    expect(conversations(input)[0].messages.map((message) => message.body)).toEqual(["Earlier", "Later"]);
    expect(conversations(input)[0].chronological).toBe(true);
    const epoch = table(["Sender", "Message", { key: "Timestamp", label: "Timestamp (seconds)" }], [
      { Sender: "Alice", Message: "Second", Timestamp: 1 }, { Sender: "Alice", Message: "First", Timestamp: 0 },
    ]);
    const mapping = detectMessageColumns(epoch.columns)!;
    const ordered = getArtifactConversations(epoch, mapping)[0];
    expect(ordered.messages.map((message) => message.body)).toEqual(["First", "Second"]);
    expect(formatMessageTime(ordered.messages[0], mapping.timestamp, "UTC")).toContain("1970-01-01 00:00:00 UTC");
    epoch.columns[2] = { key: "Timestamp", label: "Timestamp" };
    expect(conversations(epoch)[0].chronological).toBe(false);
    expect(conversations(epoch)[0].messages.map((message) => message.body)).toEqual(["Second", "First"]);
  });

  test("does not silently compare unspecified, missing or invalid clocks with absolute timestamps", () => {
    const input = table(["sender", "body", "sentAt"], [
      { sender: "Alice", body: "First row", sentAt: "2026-01-01T12:00:00Z" },
      { sender: "Alice", body: "Wall clock", sentAt: "2026-01-01 01:00:00" },
      { sender: "Alice", body: "Missing" },
    ]);
    const result = conversations(input)[0];
    expect(result.chronological).toBe(false);
    expect(result.messages.map((message) => message.rowIndex)).toEqual([0, 1, 2]);
    expect(formatMessageTime(result.messages[1], detectMessageColumns(input.columns)!.timestamp, "America/New_York")).toBe("2026-01-01 01:00:00");
  });

  test("uses explicit direction and from-me flags without treating delivery status or arbitrary numeric codes as direction", () => {
    const input = table(["sender", "body", "From Me", "Direction", "Message Sent"], [
      { sender: "Alice", body: "A", "From Me": true }, { sender: "Alice", body: "B", "From Me": "0" },
      { sender: "Alice", body: "C", Direction: "Outgoing" }, { sender: "Alice", body: "D", Direction: 1, "Message Sent": true },
    ]);
    expect(conversations(input)[0].messages.map((message) => message.direction)).toEqual(["outgoing", "incoming", "outgoing", "unknown"]);
  });

  test("renders escaped chat content and attachment references without loading source HTML or remote media", () => {
    const input = table(["Sender", "Receiver", "Message", "Attachment File", "From Me"], [
      { Sender: "Alice", Receiver: "Bob", Message: "<img src=x onerror=alert(1)>", "From Me": 0 },
      { Sender: "Bob", Receiver: "Alice", Message: "", "Attachment File": "https://example.invalid/photo.jpg", "From Me": 1 },
    ]);
    const before = JSON.stringify(input);
    const html = renderToStaticMarkup(<ArtifactMessagePreview table={input} columns={detectMessageColumns(input.columns)!} sourceArtifacts={[]} pageOnly />);
    expect(html).toContain("Messages on this page");
    expect(html).toContain("&lt;img src=x onerror=alert(1)&gt;");
    expect(html).not.toContain("<img ");
    expect(html).not.toContain('src="https://example.invalid');
    expect(html).toContain("Attachment message");
    expect(html).toContain("justify-end");
    expect(html).toContain("justify-start");
    expect(JSON.stringify(input)).toBe(before);
  });
});
