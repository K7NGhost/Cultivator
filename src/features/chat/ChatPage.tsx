import { useEffect, useRef, useState } from "react";
import { useLocation, Link } from "react-router-dom";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ArrowUp, Bot, Check, Copy, LoaderCircle, MessageSquare, PanelLeftClose, PanelLeftOpen, Plus, Square, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { useCases } from "@/features/cases";
import { useTimeZone } from "@/features/settings/timeZoneSettings";
import { IndexedRecordPreview } from "@/features/search/components/IndexedRecordPreview";
import { indexedRecordDisplay } from "@/features/search/utils/indexedRecordDisplay";
import { ChatAnswer } from "./ChatAnswer";
import { deleteConversation, listConversations, loadConversation, stopChat, type ChatEvent, type ChatMessage, type ChatReply, type Conversation, type EvidenceSource } from "./chatApi";

export function ChatPage() {
  const { activeCase } = useCases();
  return <ChatSession key={activeCase?.databasePath ?? "none"} database={activeCase?.databasePath ?? null} caseName={activeCase?.name ?? "No case open"} />;
}

function ChatSession({ database, caseName }: { database: string | null; caseName: string }) {
  const active = useLocation().pathname === "/chat";
  const timeZone = useTimeZone();
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [conversation, setConversation] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [question, setQuestion] = useState("");
  const [stage, setStage] = useState("all");
  const [semantic, setSemantic] = useState(true);
  const [sidebar, setSidebar] = useState(true);
  const [qwenReady, setQwenReady] = useState(false);
  const [bgeReady, setBgeReady] = useState(false);
  const [listening, setListening] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loadingHistory, setLoadingHistory] = useState(false);
  const [phase, setPhase] = useState("");
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<EvidenceSource | null>(null);
  const [copied, setCopied] = useState("");
  const [deleteTarget,setDeleteTarget]=useState<Conversation | null>(null);
  const request = useRef<string | null>(null);
  const alive = useRef(true);
  const historyVersion = useRef(0);
  const scroll = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const draft = useRef("");
  const frame = useRef<number | null>(null);

  useEffect(() => { alive.current = true; return () => { alive.current = false; historyVersion.current++; if (request.current) void stopChat(request.current); if (frame.current !== null) cancelAnimationFrame(frame.current); }; }, []);
  useEffect(() => {
    if (!database || !isTauri()) return;
    let disposed = false;
    let remove: (() => void) | undefined;
    const subscription = listen<ChatEvent>("case-chat-event", ({ payload }) => {
      if (disposed || payload.database !== database || payload.requestId !== request.current) return;
      const event = payload.event;
      if (event.phase) setPhase(event.phase);
      if (event.conversationId) setConversation(event.conversationId);
      if (event.sources) setMessages(previous => previous.map(message => message.id === payload.requestId ? { ...message, sources: event.sources! } : message));
      if (event.used) setMessages(previous => previous.map(message => message.id === payload.requestId ? { ...message, sources: message.sources.filter(source => event.used!.includes(source.id)) } : message));
      if (event.delta) {
        draft.current += event.delta;
        setPhase("Writing response");
        if (frame.current === null) frame.current = requestAnimationFrame(() => { frame.current = null; const body = draft.current; if (alive.current) setMessages(previous => previous.map(message => message.id === payload.requestId ? { ...message, body } : message)); });
      }
    });
    void subscription.then(unlisten => { if (disposed) unlisten(); else { remove = unlisten; setListening(true); } }).catch(error => { if (!disposed) setError(String(error)); });
    return () => { disposed = true; remove?.(); };
  }, [database]);

  useEffect(() => {
    if (!active || !database || !isTauri()) return;
    let disposed = false;
    const refresh = async () => {
      try {
        const models = await invoke<{ models: { id: string; ready: boolean }[] }>("plugin_model_settings");
        if (!disposed) { setQwenReady(models.models.some(model => model.id === "qwen3-8b-q4-k-m" && model.ready)); setBgeReady(models.models.some(model => model.id === "bge-large-en-v1.5" && model.ready)); }
        const result = await listConversations(database);
        if (!disposed) setConversations(result.conversations);
      } catch (error) { if (!disposed && !String(error).includes("restricted to the active case")) setError(String(error)); }
    };
    void refresh(); const timer = window.setInterval(() => void refresh(), 5000);
    return () => { disposed = true; clearInterval(timer); };
  }, [active, database]);
  useEffect(() => { if (follow.current) scroll.current?.scrollTo({ top: scroll.current.scrollHeight }); }, [messages, phase]);

  async function open(id: string) {
    if (!database || busy) return;
    const version = ++historyVersion.current; setLoadingHistory(true); setError("");
    try { const result = await loadConversation(database, id); if (alive.current && version === historyVersion.current) { setConversation(id); setMessages(result.messages); setSelected(null); follow.current = true; } }
    catch (error) { if (alive.current) setError(String(error)); }
    finally { if (alive.current && version === historyVersion.current) setLoadingHistory(false); }
  }
  function newChat() { historyVersion.current++; setLoadingHistory(false); setConversation(null); setMessages([]); setSelected(null); setError(""); setQuestion(""); }
  async function send() {
    if (!database || request.current || busy || loadingHistory || !question.trim() || !qwenReady || !listening) return;
    if (new TextEncoder().encode(question).length > 4096) { setError("Keep the question within 4096 UTF-8 bytes."); return; }
    const id = crypto.randomUUID(); const body = question.trim(); request.current = id; draft.current = ""; follow.current = true;
    setBusy(true); setError(""); setQuestion(""); setSelected(null); setPhase("Searching case evidence");
    setMessages(previous => [...previous, { id: `${id}-user`, role: "user", body, sources: [], warnings: [] }, { id, role: "assistant", body: "", sources: [], warnings: [] }]);
    try {
      const result = await invoke<ChatReply>("ask_case_chat", { request: { database, requestId: id, conversationId: conversation, question: body, stage: stage === "all" ? null : Number(stage), semantic } });
      if (!alive.current || request.current !== id) return;
      if (frame.current !== null) { cancelAnimationFrame(frame.current); frame.current = null; }
      setConversation(result.conversationId); setMessages(previous => previous.map(message => message.id === id ? result.message : message));
      const history = await listConversations(database); if (alive.current) setConversations(history.conversations);
    } catch (error) {
      if (frame.current !== null) { cancelAnimationFrame(frame.current); frame.current = null; }
      if (alive.current) { setError(String(error)); setMessages(previous => previous.map(message => message.id === id ? { ...message, body: "This response could not be completed.", warnings: [String(error)] } : message)); }
    } finally { if (alive.current && request.current === id) { request.current = null; setBusy(false); setPhase(""); } }
  }

  return <div className="flex h-full min-h-0 flex-col">
    <header className="flex h-9 shrink-0 items-center gap-2 border-b px-2 text-xs">
      <Button size="icon-sm" variant="ghost" aria-label="Toggle chat history" onClick={() => setSidebar(!sidebar)}>{sidebar ? <PanelLeftClose className="size-4" /> : <PanelLeftOpen className="size-4" />}</Button>
      <MessageSquare className="size-3.5" /><h1 className="font-medium">Case chat</h1><span className="truncate text-muted-foreground">{caseName}</span>
      <span className="ml-auto shrink-0 text-muted-foreground">Qwen3-8B · Local</span><Link to="/settings" className="text-primary underline underline-offset-2">Models & indexing</Link>
    </header>
    <div className="flex min-h-0 flex-1">
      {sidebar && <aside className="flex w-52 shrink-0 flex-col border-r bg-muted/10" aria-label="Chat history">
        <div className="border-b p-2"><Button size="sm" variant="outline" className="w-full justify-start" disabled={busy} onClick={newChat}><Plus className="size-3.5" />New chat</Button></div>
        <div className="min-h-0 flex-1 overflow-auto p-1">
          {!conversations.length && <p className="p-2 text-[11px] text-muted-foreground">Conversations are saved with this case.</p>}
          {conversations.map(item => <div key={item.id} className={`group flex items-center rounded-sm ${conversation === item.id ? "bg-muted" : "hover:bg-muted/50"}`}>
            <button className="min-w-0 flex-1 truncate px-2 py-2 text-left text-xs" disabled={busy || loadingHistory} title={item.title} onClick={() => void open(item.id)}>{item.title}</button>
            <Button size="icon-xs" variant="ghost" aria-label={`Delete chat: ${item.title}`} disabled={busy || loadingHistory} onClick={() => setDeleteTarget(item)}><Trash2 className="size-3" /></Button>
          </div>)}
        </div>
      </aside>}
      <main className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div ref={scroll} className="min-h-0 flex-1 overflow-auto" aria-label="Case conversation" onScroll={event => { const el = event.currentTarget; follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100; }}>
          <div className="mx-auto grid w-full max-w-3xl gap-6 px-6 py-6">
            {!messages.length && <div className="grid gap-4 py-12">
              <Bot className="size-7 text-primary" /><h2 className="text-xl font-medium">What would you like to investigate?</h2>
              <p className="max-w-xl text-sm leading-6 text-muted-foreground">Ask questions about this case. Answers use retrieved evidence and source citations you can inspect.</p>
              <div className="grid gap-2 sm:grid-cols-2">{["Which phones were connected to this vehicle?", "What evidence describes Bluetooth pairing?", "Summarize relevant messages and their participants.", "What evidence supports the sequence of events?"].map(example => <button key={example} className="rounded-lg border p-3 text-left text-xs leading-5 hover:bg-muted/40" disabled={busy} onClick={() => setQuestion(example)}>{example}</button>)}</div>
            </div>}
            {loadingHistory && <p className="text-xs text-muted-foreground">Loading conversation…</p>}
            {messages.map(message => <article key={message.id} className={message.role === "user" ? "ml-auto max-w-[85%] rounded-2xl bg-muted px-4 py-3" : "min-w-0"} aria-label={message.role === "user" ? "Investigator message" : "Assistant response"}>
              {message.role === "assistant" && <div className="mb-2 flex items-center gap-2 text-xs font-medium"><Bot className="size-4 text-primary" />Cultivator{busy && message.id === request.current && <span className="font-normal text-muted-foreground">Draft · checking citations on completion</span>}</div>}
              {message.role === "user" ? <p className="whitespace-pre-wrap break-words text-sm leading-6">{message.body}</p> : <>
                {message.body ? <ChatAnswer text={message.body} sources={message.sources} select={setSelected} /> : <div className="flex items-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{phase || "Preparing response"}</div>}
                {message.warnings.map((warning, i) => <p key={i} className="mt-2 text-xs text-amber-700 dark:text-amber-400">{warning}</p>)}
                {message.sources.length > 0 && <div className="mt-3 flex flex-wrap gap-1.5" aria-label="Evidence citations">{message.sources.map(source => <button key={source.id} className="max-w-60 truncate rounded-md border px-2 py-1.5 text-left text-[11px] hover:bg-muted" title={source.source} onClick={() => setSelected(source)}><span className="mr-1 font-medium text-primary">{source.id}</span>{source.label || source.source.split(/[\\/]/).pop() || "Evidence"}</button>)}</div>}
                {message.body && <Button size="icon-xs" variant="ghost" className="mt-2 text-muted-foreground" aria-label="Copy response" onClick={async () => { try { await navigator.clipboard.writeText(message.body); setCopied(message.id); } catch (error) { setError(String(error)); } }}>{copied === message.id ? <Check className="size-3" /> : <Copy className="size-3" />}</Button>}
              </>}
            </article>)}
          </div>
        </div>
        <div className="mx-auto w-full max-w-3xl shrink-0 px-4 pb-3">
          {(!database || !qwenReady) && <p className="mb-2 text-xs text-muted-foreground">{!database ? "Open a case to start a conversation." : <>Download Qwen3-8B in <Link to="/settings" className="text-primary underline">Settings → Models</Link> before chatting.</>}</p>}
          {semantic && !bgeReady && database && <p className="mb-2 text-[11px] text-muted-foreground">BGE Large is not downloaded. Semantic retrieval may be unavailable; keyword evidence remains available.</p>}
          {error && <p role="alert" className="mb-2 whitespace-pre-wrap text-xs text-destructive">{error}</p>}
          <form className="rounded-2xl border bg-muted/20 p-3 shadow-sm" onSubmit={event => { event.preventDefault(); void send(); }}>
            <textarea aria-label="Question about this case" className="max-h-40 min-h-16 w-full resize-y bg-transparent text-sm leading-6 outline-none placeholder:text-muted-foreground disabled:opacity-50" maxLength={4096} value={question} disabled={!database || busy} placeholder="Ask about the evidence in this case…" onChange={event => setQuestion(event.target.value)} onKeyDown={event => { if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void send(); } }} />
            <div className="flex flex-wrap items-center gap-3">
              <select aria-label="Chat evidence scope" className="max-w-40 rounded-sm border bg-background px-2 py-1 text-[11px]" value={stage} disabled={busy} onChange={event => setStage(event.target.value)}><option value="all">All indexed evidence</option><option value="1">Artifacts</option><option value="2">File contents</option><option value="0">Filesystem metadata</option></select>
              <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground"><input type="checkbox" checked={semantic} disabled={busy} onChange={event => setSemantic(event.target.checked)} />Semantic retrieval</label>
              {busy ? <Button type="button" size="icon-sm" variant="outline" className="ml-auto rounded-full" aria-label="Stop response" onClick={() => { if (request.current) { setPhase("Stopping response"); void stopChat(request.current).catch(error => setError(String(error))); } }}><Square className="size-3.5" /></Button> : <Button size="icon-sm" type="submit" className="ml-auto rounded-full" aria-label="Send question" disabled={!database || !qwenReady || !listening || loadingHistory || !question.trim()}><ArrowUp className="size-4" /></Button>}
            </div>
          </form>
          <p className="pt-2 text-center text-[10px] text-muted-foreground">AI-generated answers can be wrong. Verify cited excerpts against original evidence. Enter to send · Shift+Enter for a new line.</p>
        </div>
      </main>
      {selected && <aside className="flex w-80 min-w-0 shrink-0 flex-col border-l" aria-label="Cited evidence">
        <div className="flex h-9 shrink-0 items-center justify-between border-b px-3 text-xs font-medium">Source {selected.id}<Button size="icon-xs" variant="ghost" aria-label="Close cited evidence" onClick={() => setSelected(null)}><X className="size-3.5" /></Button></div>
        <div className="min-h-0 flex-1 overflow-auto p-3 text-xs"><p className="mb-2 break-all text-muted-foreground select-text">{selected.source}</p><p className="mb-3 break-all text-[11px] text-muted-foreground">Record: {selected.recordId || selected.group}{selected.rowIndex !== undefined && selected.rowIndex !== null ? ` · Row ${selected.rowIndex + 1}` : ""}<br />Retrieved with: {selected.methods?.join(" + ")}{selected.verified ? " · SQLite provenance verified" : ""}</p><IndexedRecordPreview hit={selected} display={indexedRecordDisplay(selected, timeZone)} /></div>
      </aside>}
    </div>
    <Dialog open={deleteTarget !== null} onOpenChange={open => { if (!open) setDeleteTarget(null); }}>
      <DialogContent><DialogHeader><DialogTitle>Delete conversation?</DialogTitle><DialogDescription>Delete “{deleteTarget?.title}” and its saved replies? Original case evidence is unaffected.</DialogDescription></DialogHeader>
        <DialogFooter><Button variant="outline" onClick={() => setDeleteTarget(null)}>Cancel</Button><Button variant="destructive" disabled={busy} onClick={async () => { if (!database || !deleteTarget) return; const id=deleteTarget.id; try { await deleteConversation(database,id); setConversations(items=>items.filter(chat=>chat.id!==id)); if(conversation===id)newChat();setDeleteTarget(null); } catch(error){setError(String(error));} }}>Delete conversation</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  </div>;
}
