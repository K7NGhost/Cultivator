import { useEffect, useMemo, useState } from "react";
import { MessageSquare, Paperclip, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useTimeZone } from "@/features/settings/timeZoneSettings";
import type { StoredArtifactRecord } from "../model/types";
import type { CustomTableView } from "../utils/customTableGrouping";
import { artifactMessageSourceScopes, formatMessageTime, getArtifactConversations, messageText, type MessageColumns } from "../utils/artifactMessages";
import { formatArtifactTableCell } from "../utils/artifactTableFormatting";

const THREAD_PAGE_SIZE = 40;
const MESSAGE_PAGE_SIZE = 100;

export function ArtifactMessagePreview({ table, columns, sourceArtifacts, selectedRowIndex, onSelectRow, onSelectArtifactId, pageOnly = false }: {
  table: CustomTableView; columns: MessageColumns; sourceArtifacts: StoredArtifactRecord[];
  selectedRowIndex?: number; onSelectRow?: (index: number) => void;
  onSelectArtifactId?: (id: string) => void; pageOnly?: boolean;
}) {
  const timeZone = useTimeZone();
  const scopes = useMemo(() => artifactMessageSourceScopes(sourceArtifacts), [sourceArtifacts]);
  const conversations = useMemo(() => getArtifactConversations(table, columns, scopes), [table, columns, scopes]);
  const [threadId, setThreadId] = useState("");
  const [query, setQuery] = useState("");
  const [threadOffset, setThreadOffset] = useState(0);
  const [messageOffset, setMessageOffset] = useState(0);
  const [perspective, setPerspective] = useState("");
  const [details, setDetails] = useState(false);
  const [localRow, setLocalRow] = useState<number | undefined>(selectedRowIndex);
  const selectedThread = conversations.find((thread) => thread.messages.some((message) => message.rowIndex === selectedRowIndex));
  const thread = conversations.find((candidate) => candidate.id === threadId) ?? selectedThread ?? conversations[0];
  const matching = useMemo(() => conversations.filter((candidate) => `${candidate.title} ${candidate.thread} ${candidate.participants.join(" ")}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())), [conversations, query]);
  const threadStart = Math.min(threadOffset, Math.floor(Math.max(0, matching.length - 1) / THREAD_PAGE_SIZE) * THREAD_PAGE_SIZE);
  const filteredThreads = matching.slice(threadStart, threadStart + THREAD_PAGE_SIZE);
  const selectedIndex = localRow ?? selectedRowIndex;
  const selectedMessage = thread?.messages.find((message) => message.rowIndex === selectedIndex) ?? thread?.messages[0];
  const row = selectedMessage ? table.rows[selectedMessage.rowIndex] : undefined;
  const source = row ? table.sources[row.primarySourceIndex] : undefined;
  const senders = thread ? [...new Set(thread.messages.map((message) => message.sender).filter(Boolean))] : [];
  const viewAs = senders.includes(perspective) ? perspective : "";

  useEffect(() => {
    if (!selectedThread) return;
    setThreadId(selectedThread.id); setLocalRow(selectedRowIndex);
    const index = selectedThread.messages.findIndex((message) => message.rowIndex === selectedRowIndex);
    setMessageOffset(Math.floor(Math.max(0, index) / MESSAGE_PAGE_SIZE) * MESSAGE_PAGE_SIZE);
  }, [selectedThread?.id, selectedRowIndex]);
  useEffect(() => {
    setPerspective("");
    const index = thread?.messages.findIndex((message) => message.rowIndex === selectedRowIndex) ?? -1;
    setMessageOffset(Math.floor(Math.max(0, index) / MESSAGE_PAGE_SIZE) * MESSAGE_PAGE_SIZE);
  }, [thread?.id]);

  if (!thread) return <div role="status" className="grid flex-1 place-items-center p-3 text-xs text-muted-foreground">No messages in this view.</div>;
  const chooseThread = (id: string) => { setThreadId(id); setLocalRow(undefined); setMessageOffset(0); };
  const messageStart = Math.min(messageOffset, Math.floor(Math.max(0, thread.messages.length - 1) / MESSAGE_PAGE_SIZE) * MESSAGE_PAGE_SIZE);
  const visibleMessages = thread.messages.slice(messageStart, messageStart + MESSAGE_PAGE_SIZE);
  return <section className="flex h-full min-h-0 min-w-0 flex-col" aria-label="Message conversation preview">
    <header className="flex shrink-0 flex-wrap items-center gap-2 border-b px-2 py-1.5 text-xs">
      <MessageSquare className="size-3.5 text-primary" /><span className="font-medium">Messages{pageOnly ? " on this page" : ""}</span>
      <span className="text-[11px] text-muted-foreground">{table.rows.length.toLocaleString()} messages · {conversations.length.toLocaleString()} conversations</span>
      <details className="ml-auto text-[11px]"><summary className="cursor-pointer text-muted-foreground">Detected columns</summary><dl className="space-y-1 py-2">{Object.entries(columns).map(([role, column]) => <div key={role} className="flex gap-2"><dt className="capitalize text-muted-foreground">{role}</dt><dd>{column?.label}</dd></div>)}</dl></details>
    </header>
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-52 shrink-0 flex-col border-r" aria-label="Message conversations">
        <div className="relative m-2 shrink-0"><Search className="pointer-events-none absolute left-2 top-2 size-3 text-muted-foreground" /><Input aria-label="Find a conversation" placeholder="Find a conversation…" className="h-7 pl-7 text-xs" value={query} onChange={(event) => { setQuery(event.target.value); setThreadOffset(0); }} /></div>
        <div className="min-h-0 flex-1 overflow-y-auto">{filteredThreads.map((candidate) => <button type="button" key={candidate.id} aria-pressed={candidate.id === thread.id} onClick={() => chooseThread(candidate.id)}
          className={`block w-full space-y-1 border-b px-2 py-2 text-left text-xs text-foreground hover:bg-muted/40 ${candidate.id === thread.id ? "bg-primary/10" : ""}`}>
          <span className="block truncate font-medium" title={candidate.title}>{candidate.title}</span>
          <span className="block truncate text-[10px] text-muted-foreground" title={candidate.sourcePath}>{candidate.sourcePath || "Source not recorded"}</span>
          <span className="block text-[10px] text-muted-foreground">{candidate.messages.length.toLocaleString()} messages</span>
        </button>)}{!matching.length && <p className="p-2 text-xs text-muted-foreground">No matching conversations.</p>}</div>
        {matching.length > THREAD_PAGE_SIZE && <div className="flex justify-between border-t p-1"><Button size="xs" variant="ghost" disabled={!threadStart} onClick={() => setThreadOffset(threadStart - THREAD_PAGE_SIZE)}>Previous</Button><Button size="xs" variant="ghost" disabled={threadStart + THREAD_PAGE_SIZE >= matching.length} onClick={() => setThreadOffset(threadStart + THREAD_PAGE_SIZE)}>Next</Button></div>}
      </aside>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b px-3 py-2 text-xs">
          <div className="min-w-0 flex-1"><h3 className="truncate font-semibold" title={thread.title}>{thread.title}</h3><p className="mt-0.5 text-[10px] text-muted-foreground">{thread.basis === "thread" ? `Thread: ${thread.thread}` : thread.basis === "participants" ? "Grouped by recorded participants" : thread.basis === "unassigned" ? "No recorded conversation or participants" : "Grouped by chat name"} · {thread.chronological ? "Chronological order" : "Recorded row order"}</p></div>
          <select aria-label="Chat perspective" className="max-w-40 rounded-sm border bg-background px-1 py-1 text-[11px]" value={viewAs} onChange={(event) => setPerspective(event.target.value)}><option value="">Recorded direction</option>{senders.map((sender) => <option key={sender} value={sender}>View as {sender}</option>)}</select>
          <Button type="button" variant="outline" size="xs" aria-pressed={details} onClick={() => setDetails(!details)}>{details ? "Hide record" : "Show record"}</Button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto bg-muted/15 p-3" aria-label="Conversation messages">
          <ol className="space-y-3">{visibleMessages.map((message) => {
            const outgoing = viewAs ? message.sender === viewAs : message.direction === "outgoing";
            const timestamp = formatMessageTime(message, columns.timestamp, timeZone);
            return <li key={message.rowIndex} className={`flex ${outgoing ? "justify-end" : "justify-start"}`}>
              <button type="button" aria-pressed={selectedMessage?.rowIndex === message.rowIndex} onClick={() => { setLocalRow(message.rowIndex); onSelectRow?.(message.rowIndex); }}
                className={`max-w-[85%] space-y-1 rounded-lg border px-3 py-2 text-left text-xs text-foreground select-text ${outgoing ? "border-primary/20 bg-primary/10" : "bg-background"} ${selectedMessage?.rowIndex === message.rowIndex ? "ring-1 ring-primary/40" : ""}`}>
                <span className="block break-all font-semibold">{message.senderLabel}</span>
                {message.subject && <span className="block font-medium">{message.subject}</span>}
                <span className="block whitespace-pre-wrap break-words leading-5">{message.body || (message.attachment ? "Attachment message" : "(empty message)")}</span>
                {message.attachment && <span className="flex items-start gap-1 whitespace-pre-wrap break-all text-[11px]"><Paperclip className="mt-0.5 size-3 shrink-0" />{message.attachment}</span>}
                {message.attachmentBytes && <span className="block text-[10px] text-muted-foreground">Attachment bytes: {message.attachmentBytes}</span>}
                {message.recipients.length > 0 && <span className="block break-words text-[10px] text-muted-foreground">To: {message.recipients.join(", ")}</span>}
                <span className="flex flex-wrap gap-x-2 text-[10px] text-muted-foreground">{timestamp && <time>{timestamp}</time>}{message.direction !== "unknown" && <span>{message.direction}</span>}{message.messageType && <span>{message.messageType}</span>}{message.service && <span>{message.service}</span>}{message.status && <span>{message.status}</span>}</span>
              </button>
            </li>;
          })}</ol>
        </div>
        {thread.messages.length > MESSAGE_PAGE_SIZE && <div className="flex shrink-0 items-center justify-between border-t p-1 text-[10px]"><Button size="xs" variant="ghost" disabled={!messageStart} onClick={() => setMessageOffset(messageStart - MESSAGE_PAGE_SIZE)}>Previous messages</Button><span>{messageStart + 1}–{Math.min(messageStart + MESSAGE_PAGE_SIZE, thread.messages.length)} / {thread.messages.length}</span><Button size="xs" variant="ghost" disabled={messageStart + MESSAGE_PAGE_SIZE >= thread.messages.length} onClick={() => setMessageOffset(messageStart + MESSAGE_PAGE_SIZE)}>Next messages</Button></div>}
        {details && selectedMessage && row && <section className="max-h-[40%] shrink-0 overflow-y-auto border-t p-2 text-xs" aria-label="Message record details">
          <p className="mb-2 break-all text-[11px] text-muted-foreground">Row {selectedMessage.rowIndex + 1} · {row.occurrenceCount} occurrences · Primary source: {source?.filePath || "Not recorded"}</p>
          {source && onSelectArtifactId && <Button size="xs" variant="outline" className="mb-2" onClick={() => onSelectArtifactId(source.artifactId)}>Open source artifact</Button>}
          <dl className="divide-y">{table.columns.map((column) => <div key={column.key} className="grid grid-cols-[minmax(6rem,30%)_1fr] gap-2 py-1"><dt className="text-muted-foreground">{column.label}</dt><dd className="whitespace-pre-wrap break-all select-text">{formatArtifactTableCell(row.values[column.key], column, timeZone) || messageText(row.values[column.key]) || "—"}</dd></div>)}</dl>
        </section>}
      </div>
    </div>
  </section>;
}
