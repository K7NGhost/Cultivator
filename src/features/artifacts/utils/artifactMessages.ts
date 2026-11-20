import type { CustomTableColumn, StoredArtifactRecord } from "../model/types";
import type { CustomTableView } from "./customTableGrouping";
import { parseTimestamp } from "@/lib/timeZone";
import { formatArtifactTableCell } from "./artifactTableFormatting";

export type MessageColumns = Partial<Record<"body" | "sender" | "senderName" | "recipients" | "timestamp" | "direction" | "fromMe" | "conversation" | "conversationName" | "participants" | "attachment" | "attachmentBytes" | "messageType" | "service" | "subject" | "status", CustomTableColumn>> & { body: CustomTableColumn };

const aliases = {
  body: ["body", "messagetext", "messagebody", "message", "text", "textbody", "content", "ztext"],
  sender: ["sender", "messagesender", "from", "senderid", "fromid", "senderaddress", "fromaddress", "sendernumber", "fromjid", "zfromjid", "sendername", "author", "authorid", "authorname"],
  senderName: ["senderdisplayname", "sendername", "fromname", "authorname"],
  recipients: ["recipients", "recipient", "receiver", "receivers", "to", "recipientid", "receiverid", "toaddress", "recipientaddress", "recipientname", "receivername", "destinationcallerid", "chatcontactid"],
  timestamp: ["sentat", "messagetimestamp", "messagedatetime", "senttime", "datesent", "messagedate", "timestamp", "timestamputc", "timestampseconds", "timestampmilliseconds", "datetime", "date", "time", "createdat", "receivedat"],
  direction: ["direction", "messagedirection", "senddirection"],
  fromMe: ["isfromme", "fromme", "isoutgoing", "outgoing", "fromowner"],
  conversation: ["conversationid", "threadid", "chatid", "dialogid", "roomid", "threadidentifier", "conversationidentifier", "conversation"],
  conversationName: ["conversationname", "otherparticipantconversationname", "otherparticipant", "chatname", "threadname", "grouptitle", "groupname", "chattitle", "conversationtitle"],
  participants: ["chatparticipants", "participants", "members", "conversationparticipants"],
  attachment: ["attachments", "attachmentfile", "attachmentlocation", "attachment", "attachmentpath", "attachmentname", "mediafile"],
  attachmentBytes: ["attachmentbytesize", "attachmentsizebytes", "attachmentbytes"],
  messageType: ["messagetype"],
  service: ["service", "application", "app", "messagingservice", "protocol"],
  subject: ["subject", "messagesubject"],
  status: ["status", "messagestatus", "deliverystatus", "readstatus"],
} as const;

const normalize = (value: string) => value.replace(/\([^)]*\)|\[[^\]]*\]/g, "").toLowerCase().replace(/[^a-z0-9]/g, "");

export function detectMessageColumns(columns: CustomTableColumn[]): MessageColumns | null {
  const found: Partial<Record<keyof typeof aliases, CustomTableColumn>> = {};
  for (const [role, names] of Object.entries(aliases) as [keyof typeof aliases, readonly string[]][]) {
    let best = Infinity;
    for (const column of columns) {
      for (const candidate of [column.key, column.label]) {
        const rank = names.indexOf(normalize(candidate));
        if (rank >= 0 && rank < best) { found[role] = column; best = rank; }
      }
    }
  }
  if (!found.body) return null;
  // An author and a body alone can describe a note or social post. Require
  // stronger message evidence before assigning those a conversation preview.
  const strongSender = found.sender && [found.sender.key, found.sender.label].some((name) => aliases.sender.includes(normalize(name) as typeof aliases.sender[number]) && !/^author/.test(normalize(name)));
  if (!found.recipients && !strongSender && !(found.fromMe && (found.conversation || found.participants))) return null;
  if (found.timestamp && !found.timestamp.timestampUnit) {
    const hints = `${found.timestamp.key} ${found.timestamp.label}`.toLowerCase();
    const unit = /milliseconds?|\bms\b/.test(hints) ? "milliseconds" : /seconds?|\bsecs?\b/.test(hints) ? "seconds" : undefined;
    if (unit) found.timestamp = { ...found.timestamp, timestampUnit: unit };
  }
  return found as MessageColumns;
}

export function messageText(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (Array.isArray(value)) return value.map(messageText).filter(Boolean).join(", ");
  if (typeof value === "object") return Object.entries(value).map(([key, item]) => `${key}: ${messageText(item)}`).join("\n");
  return String(value);
}

function people(value: unknown): string[] {
  if (Array.isArray(value)) return value.flatMap(people);
  if (typeof value === "string" && value.trim().startsWith("[")) {
    try { const parsed: unknown = JSON.parse(value); if (Array.isArray(parsed)) return parsed.flatMap(people); } catch { /* Keep source text intact. */ }
  }
  const text = messageText(value).trim();
  return text ? [text] : [];
}

export type MessageDirection = "incoming" | "outgoing" | "unknown";
function messageDirection(values: Record<string, unknown>, columns: MessageColumns): MessageDirection {
  const flag = columns.fromMe ? values[columns.fromMe.key] : undefined;
  if (flag === true || flag === 1 || /^(true|yes|1)$/i.test(String(flag))) return "outgoing";
  if (flag === false || flag === 0 || /^(false|no|0)$/i.test(String(flag))) return "incoming";
  const direction = normalize(messageText(columns.direction ? values[columns.direction.key] : undefined));
  if (["outgoing", "outbound", "sent", "send", "out", "outgoingmessage"].includes(direction)) return "outgoing";
  if (["incoming", "inbound", "received", "recv", "in", "incomingmessage"].includes(direction)) return "incoming";
  return "unknown";
}

export type ArtifactChatMessage = {
  rowIndex: number; sender: string; senderLabel: string; recipients: string[]; body: string;
  direction: MessageDirection; timestamp: unknown; time: number | null; timeBasis: string | null;
  attachment: string; attachmentBytes: string; messageType: string; subject: string; service: string; status: string;
};
export type ArtifactConversation = {
  id: string; title: string; participants: string[]; messages: ArtifactChatMessage[];
  basis: "thread" | "name" | "participants" | "unassigned";
  thread: string; sourcePath: string; chronological: boolean;
};

export function artifactMessageSourceScopes(artifacts: StoredArtifactRecord[]) {
  return new Map(artifacts.map((artifact) => [artifact.id, JSON.stringify([artifact.datasourceId, artifact.filePath.replace(/\\/g, "/") || artifact.id])]));
}

function messageTime(value: unknown, column?: CustomTableColumn): { time: number; basis: string } | null {
  const parsed = parseTimestamp(value);
  if (parsed) return parsed;
  if (column?.timestampUnit && (typeof value === "number" || typeof value === "string" && /^[+-]?\d+(?:\.\d+)?$/.test(value.trim()))) {
    const numeric = Number(value);
    const time = numeric * (column.timestampUnit === "seconds" ? 1000 : 1);
    if (Number.isFinite(numeric) && Number.isFinite(new Date(time).getTime())) return { time, basis: "zoned" };
  }
  return null;
}

export function formatMessageTime(message: ArtifactChatMessage, column: CustomTableColumn | undefined, timeZone: string): string {
  return column ? formatArtifactTableCell(message.timestamp, { ...column, format: column.format ?? "datetime" }, timeZone) : "";
}

export function getArtifactConversations(table: CustomTableView, columns: MessageColumns, sourceScopes?: ReadonlyMap<string, string>): ArtifactConversation[] {
  const groups = new Map<string, ArtifactConversation>();
  const value = (values: Record<string, unknown>, role: keyof MessageColumns) => columns[role] ? values[columns[role].key] : undefined;
  table.rows.forEach((row, rowIndex) => {
    const values = row.values;
    const sender = messageText(value(values, "sender")).trim();
    const recipients = people(value(values, "recipients"));
    const explicitParticipants = people(value(values, "participants"));
    const participants = [...new Set(explicitParticipants.length ? explicitParticipants : [sender, ...recipients].filter(Boolean))].sort();
    const thread = messageText(value(values, "conversation")).trim();
    const name = messageText(value(values, "conversationName")).trim();
    const source = table.sources[row.primarySourceIndex];
    const scope = source ? sourceScopes?.get(source.artifactId) ?? (source.filePath.replace(/\\/g, "/") || source.artifactId) : `unknown:${row.primarySourceIndex}`;
    const basis = thread ? "thread" : name ? "name" : participants.length ? "participants" : "unassigned";
    const service = messageText(value(values, "service"));
    const id = JSON.stringify([scope, basis, thread || name || participants, thread ? "" : service]);
    const group: ArtifactConversation = groups.get(id) ?? { id, title: name || (participants.length ? participants.join(" ↔ ") : thread ? `Conversation ${thread}` : "Unassigned messages"),
      thread, basis, participants: [], sourcePath: source?.filePath ?? "", messages: [], chronological: false };
    group.participants = [...new Set([...group.participants, ...participants])];
    const timestamp = value(values, "timestamp");
    const time = messageTime(timestamp, columns.timestamp);
    group.messages.push({ rowIndex, sender, senderLabel: messageText(value(values, "senderName")).trim() || sender || "Unknown sender",
      recipients, body: messageText(value(values, "body")), direction: messageDirection(values, columns),
      timestamp, time: time?.time ?? null, timeBasis: time?.basis ?? null, attachment: messageText(value(values, "attachment")),
      attachmentBytes: messageText(value(values, "attachmentBytes")), messageType: messageText(value(values, "messageType")),
      subject: messageText(value(values, "subject")), service, status: messageText(value(values, "status")) });
    groups.set(id, group);
  });
  for (const group of groups.values()) {
    group.chronological = group.messages.every((message) => message.time !== null && message.timeBasis === group.messages[0].timeBasis);
    if (group.chronological) group.messages.sort((a, b) => a.time! - b.time! || a.rowIndex - b.rowIndex);
  }
  return [...groups.values()];
}
