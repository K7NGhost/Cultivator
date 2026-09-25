import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useLocation } from "react-router-dom";
import {
  AlertCircle,
  Calendar,
  Contact,
  Film,
  Image as ImageIcon,
  Info,
  MessageSquare,
  Phone,
  RefreshCw,
  SendHorizontal,
  Sparkles,
  Tag,
  User,
  X,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { listArtifacts } from "@/features/artifacts/artifactRepository";
import {
  indexAssistantData,
  runAssistantQuery,
  type AssistantCall,
  type AssistantContact,
  type AssistantIndex,
  type AssistantMedia,
  type AssistantMessage,
  type AssistantResults,
} from "@/features/assistant/assistantEngine";
import {
  parseAssistantQuery,
  splitParticipant,
  type AssistantQuery,
  type AssistantTarget,
} from "@/features/assistant/assistantQuery";
import { useCases } from "@/features/cases/case-provider";
import { MediaImagePreview } from "@/features/media/components/MediaImagePreview";
import { MediaVideoPreview } from "@/features/media/components/MediaVideoPreview";
import { listMediaGallery } from "@/features/media/mediaRepository";
import { cn } from "@/lib/utils";

type AssistantEntry = {
  id: string;
  query: AssistantQuery;
  results: AssistantResults;
};

type PersonRole = "with" | "from" | "to";

const MAX_ROWS = 200;
const TARGET_LABELS: Record<AssistantTarget, string> = {
  messages: "Messages",
  calls: "Calls",
  contacts: "Contacts",
  photos: "Photos",
  videos: "Videos",
};
const ROLE_LABELS: Record<PersonRole, string> = { with: "With", from: "From", to: "To" };
const DATE_TIME_FORMAT = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

let nextEntryId = 0;

export function AssistantPage() {
  const { activeCase } = useCases();
  const location = useLocation();
  const isActive = location.pathname === "/assistant";
  const [index, setIndex] = useState<AssistantIndex | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [input, setInput] = useState("");
  const [entries, setEntries] = useState<AssistantEntry[]>([]);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  // Reload whenever the page is shown, so results reflect plugins that
  // finished while the examiner was elsewhere.
  useEffect(() => {
    if (!activeCase) {
      setIndex(null);
      setEntries([]);
      return;
    }

    if (!isActive) {
      return;
    }

    let isCurrent = true;
    setIsLoading(true);
    setError(null);

    Promise.all([
      listArtifacts(activeCase.databasePath),
      listMediaGallery(activeCase.databasePath, null),
    ])
      .then(([artifacts, gallery]) => {
        if (!isCurrent) {
          return;
        }
        const nextIndex = indexAssistantData(artifacts, [...gallery.photos, ...gallery.videos]);
        setIndex(nextIndex);
        setEntries((current) =>
          current.map((entry) => ({ ...entry, results: runAssistantQuery(entry.query, nextIndex) })),
        );
      })
      .catch((caughtError) => {
        if (isCurrent) {
          setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
          setIndex(null);
        }
      })
      .finally(() => {
        if (isCurrent) {
          setIsLoading(false);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [activeCase, isActive, refreshKey]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [entries.length]);

  const examples = useMemo(() => buildExamples(index), [index]);

  function ask(text: string) {
    if (!index || !text.trim()) {
      return;
    }
    const query = parseAssistantQuery(text.trim(), { people: index.people });
    setEntries((current) => [
      ...current,
      { id: `entry-${nextEntryId++}`, query, results: runAssistantQuery(query, index) },
    ]);
    setInput("");
  }

  function updateQuery(entryId: string, update: (query: AssistantQuery) => AssistantQuery) {
    if (!index) {
      return;
    }
    setEntries((current) =>
      current.map((entry) => {
        if (entry.id !== entryId) {
          return entry;
        }
        const query = { ...update(entry.query), notes: [] };
        return { ...entry, query, results: runAssistantQuery(query, index) };
      }),
    );
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    ask(input);
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <Sparkles className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <h1 className="text-sm font-semibold">Assistant</h1>
        <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
          Offline · rule-based
        </Badge>
        <Separator orientation="vertical" className="h-5" />
        <div className="flex min-w-0 items-center gap-2 truncate text-[11px] text-muted-foreground">
          {index ? (
            <>
              <span>Messages: {index.messages.length.toLocaleString()}</span>
              <span>Calls: {index.calls.length.toLocaleString()}</span>
              <span>Contacts: {index.contacts.length.toLocaleString()}</span>
              <span>Photos: {index.photos.length.toLocaleString()}</span>
              <span>Videos: {index.videos.length.toLocaleString()}</span>
            </>
          ) : (
            <span>{activeCase ? "Loading case data..." : "No case open"}</span>
          )}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="ml-auto h-7 rounded-sm px-2 text-xs"
          disabled={!activeCase || isLoading}
          onClick={() => setRefreshKey((key) => key + 1)}
        >
          <RefreshCw className={cn("size-3.5", isLoading && "animate-spin")} />
          Refresh
        </Button>
      </section>

      {error && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{error}</span>
        </section>
      )}

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto">
        <div className="mx-auto flex max-w-6xl flex-col gap-4 p-3">
          {entries.length === 0 ? (
            <EmptyState
              examples={examples}
              hasCase={Boolean(activeCase)}
              index={index}
              onAsk={ask}
            />
          ) : (
            entries.map((entry) => (
              <EntryView
                key={entry.id}
                caseDatabasePath={activeCase?.databasePath ?? null}
                entry={entry}
                onUpdate={(update) => updateQuery(entry.id, update)}
              />
            ))
          )}
        </div>
      </div>

      <form className="shrink-0 border-t p-2" onSubmit={handleSubmit}>
        <div className="mx-auto flex max-w-6xl gap-2">
          <Input
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder={
              activeCase
                ? 'Ask about this case, e.g. "messages between John and Jane March - Jun 2025"'
                : "Open or create a case first"
            }
            disabled={!activeCase}
            className="h-8 rounded-sm text-xs"
            aria-label="Ask the assistant"
          />
          <Button
            type="submit"
            size="xs"
            className="h-8 rounded-sm px-3 text-xs"
            disabled={!index || !input.trim()}
          >
            <SendHorizontal className="size-3.5" aria-hidden="true" />
            Ask
          </Button>
        </div>
      </form>
    </div>
  );
}

function EmptyState({
  examples,
  hasCase,
  index,
  onAsk,
}: {
  examples: string[];
  hasCase: boolean;
  index: AssistantIndex | null;
  onAsk: (text: string) => void;
}) {
  const hasData =
    index &&
    index.messages.length + index.calls.length + index.contacts.length + index.photos.length + index.videos.length > 0;

  return (
    <div className="mx-auto mt-6 flex max-w-2xl flex-col gap-3 text-xs">
      <div className="flex items-center gap-2">
        <Sparkles className="size-4 text-muted-foreground" aria-hidden="true" />
        <h2 className="text-sm font-semibold">Ask questions about this case</h2>
      </div>
      <p className="text-muted-foreground">
        Questions are turned into a filter over messages, calls, contacts, and photos (by
        AI caption and tags). The filter is shown with every answer, so you can check exactly what
        was searched and remove any part of it. Everything runs on this machine.
      </p>
      {!hasCase && <p className="text-muted-foreground">Open or create a case to start.</p>}
      {hasCase && index && !hasData && (
        <p className="rounded-sm border px-2 py-1.5 text-muted-foreground">
          No searchable data yet. Run plugins on a datasource first: a message parser (e.g. JSON
          Chat Export) for messages, calls, and contacts, and Image Metadata + Media Tagging (AI)
          for photos.
        </p>
      )}
      <div className="flex flex-wrap gap-1.5">
        {examples.map((example) => (
          <Button
            key={example}
            type="button"
            variant="outline"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={!index}
            onClick={() => onAsk(example)}
          >
            {example}
          </Button>
        ))}
      </div>
    </div>
  );
}

function EntryView({
  caseDatabasePath,
  entry,
  onUpdate,
}: {
  caseDatabasePath: string | null;
  entry: AssistantEntry;
  onUpdate: (update: (query: AssistantQuery) => AssistantQuery) => void;
}) {
  const { query, results } = entry;
  const summary = summarize(results);

  return (
    <article className="flex flex-col gap-2 rounded-sm border p-2">
      <div className="flex items-start gap-2 text-xs">
        <User className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <p className="font-medium">{query.text}</p>
      </div>

      <FilterChips query={query} onUpdate={onUpdate} />

      {results.notes.map((note) => (
        <p key={note} className="flex items-start gap-1.5 text-[11px] text-muted-foreground">
          <Info className="mt-0.5 size-3 shrink-0" aria-hidden="true" />
          {note}
        </p>
      ))}

      <p className="text-xs text-muted-foreground">{summary}</p>

      {results.searched.includes("messages") && (
        <ResultSection icon={MessageSquare} title="Messages" count={results.messages.length}>
          <MessageList messages={results.messages} keywords={query.keywords} />
        </ResultSection>
      )}
      {results.searched.includes("calls") && (
        <ResultSection icon={Phone} title="Calls" count={results.calls.length}>
          <CallList calls={results.calls} />
        </ResultSection>
      )}
      {results.searched.includes("contacts") && (
        <ResultSection icon={Contact} title="Contacts" count={results.contacts.length}>
          <ContactList contacts={results.contacts} />
        </ResultSection>
      )}
      {results.searched.includes("photos") && (
        <ResultSection icon={ImageIcon} title="Photos" count={results.photos.length}>
          <MediaGrid caseDatabasePath={caseDatabasePath} entries={results.photos} keywords={query.keywords} />
        </ResultSection>
      )}
      {results.searched.includes("videos") && (
        <ResultSection icon={Film} title="Videos" count={results.videos.length}>
          <MediaGrid caseDatabasePath={caseDatabasePath} entries={results.videos} keywords={query.keywords} />
        </ResultSection>
      )}
    </article>
  );
}

function FilterChips({
  query,
  onUpdate,
}: {
  query: AssistantQuery;
  onUpdate: (update: (query: AssistantQuery) => AssistantQuery) => void;
}) {
  const people = (["with", "from", "to"] as const).flatMap((role) =>
    query.people[role].map((person, index) => ({ role, person, index })),
  );

  return (
    <div className="flex flex-wrap items-center gap-1 text-[11px]">
      <span className="text-muted-foreground">Interpreted as:</span>
      {query.targets.length === 0 ? (
        <Chip icon={Sparkles} label="Everything" />
      ) : (
        query.targets.map((target) => (
          <Chip
            key={target}
            icon={Sparkles}
            label={TARGET_LABELS[target]}
            onRemove={() =>
              onUpdate((current) => ({
                ...current,
                targets: current.targets.filter((value) => value !== target),
              }))
            }
          />
        ))
      )}
      {people.map(({ role, person, index }) => (
        <Chip
          key={`${role}-${person.name}`}
          icon={User}
          label={`${ROLE_LABELS[role]}: ${person.name}`}
          onRemove={() =>
            onUpdate((current) => ({
              ...current,
              people: {
                ...current.people,
                [role]: current.people[role].filter((_, position) => position !== index),
              },
            }))
          }
        />
      ))}
      {query.dateRange && (
        <Chip
          icon={Calendar}
          label={query.dateRange.label}
          onRemove={() => onUpdate((current) => ({ ...current, dateRange: null }))}
        />
      )}
      {query.keywords.map((keyword) => (
        <Chip
          key={keyword}
          icon={Tag}
          label={`"${keyword}"`}
          onRemove={() =>
            onUpdate((current) => ({
              ...current,
              keywords: current.keywords.filter((value) => value !== keyword),
            }))
          }
        />
      ))}
    </div>
  );
}

function Chip({
  icon: Icon,
  label,
  onRemove,
}: {
  icon: typeof Tag;
  label: string;
  onRemove?: () => void;
}) {
  return (
    <Badge variant="secondary" className="h-5 gap-1 rounded-sm px-1.5 text-[11px] font-normal">
      <Icon className="size-3" aria-hidden="true" />
      {label}
      {onRemove && (
        <button
          type="button"
          className="-mr-0.5 rounded-sm opacity-60 hover:opacity-100"
          aria-label={`Remove filter ${label}`}
          onClick={onRemove}
        >
          <X className="size-3" />
        </button>
      )}
    </Badge>
  );
}

function ResultSection({
  children,
  count,
  icon: Icon,
  title,
}: {
  children: ReactNode;
  count: number;
  icon: typeof Tag;
  title: string;
}) {
  return (
    <section className="flex flex-col gap-1">
      <div className="flex items-center gap-1.5 text-xs font-medium">
        <Icon className="size-3.5 text-muted-foreground" aria-hidden="true" />
        {title}
        <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
          {count.toLocaleString()}
        </Badge>
        {count > MAX_ROWS && (
          <span className="text-[11px] font-normal text-muted-foreground">
            showing first {MAX_ROWS}
          </span>
        )}
      </div>
      {count === 0 ? (
        <p className="text-[11px] text-muted-foreground">No {title.toLowerCase()} match.</p>
      ) : (
        children
      )}
    </section>
  );
}

function MessageList({ messages, keywords }: { messages: AssistantMessage[]; keywords: string[] }) {
  return (
    <div className="rounded-sm border">
      {messages.slice(0, MAX_ROWS).map((message) => (
        <div
          key={message.id}
          className="grid grid-cols-[9.5rem_minmax(0,15rem)_minmax(0,1fr)] gap-2 border-b px-2 py-1 text-xs last:border-b-0"
          title={message.filePath}
        >
          <span className="font-mono text-[11px] text-muted-foreground">{formatDate(message.at)}</span>
          <span className="truncate" title={`${message.sender} → ${message.recipients.join(", ")}`}>
            <span className="font-medium">{participantName(message.sender)}</span>
            <span className="text-muted-foreground"> → </span>
            {message.recipients.map(participantName).join(", ")}
          </span>
          <span className="whitespace-pre-wrap break-words">{highlight(message.body, keywords)}</span>
        </div>
      ))}
    </div>
  );
}

function CallList({ calls }: { calls: AssistantCall[] }) {
  return (
    <div className="rounded-sm border">
      {calls.slice(0, MAX_ROWS).map((call) => (
        <div
          key={call.id}
          className="grid grid-cols-[9.5rem_6rem_minmax(0,1fr)_5rem] gap-2 border-b px-2 py-1 text-xs last:border-b-0"
          title={call.filePath}
        >
          <span className="font-mono text-[11px] text-muted-foreground">{formatDate(call.at)}</span>
          <span className="capitalize">{call.direction}</span>
          <span className="truncate">
            {call.contactName ? `${call.contactName} (${call.phone})` : call.phone}
          </span>
          <span className="text-right font-mono text-[11px]">{formatDuration(call.durationSeconds)}</span>
        </div>
      ))}
    </div>
  );
}

function ContactList({ contacts }: { contacts: AssistantContact[] }) {
  return (
    <div className="rounded-sm border">
      {contacts.slice(0, MAX_ROWS).map((contact) => (
        <div
          key={contact.id}
          className="grid grid-cols-[minmax(0,12rem)_minmax(0,10rem)_minmax(0,1fr)] gap-2 border-b px-2 py-1 text-xs last:border-b-0"
          title={contact.filePath}
        >
          <span className="truncate font-medium">{contact.name}</span>
          <span className="truncate font-mono text-[11px]">{contact.phones.join(", ") || "-"}</span>
          <span className="truncate text-muted-foreground">
            {[contact.emails.join(", "), contact.notes].filter(Boolean).join(" · ") || "-"}
          </span>
        </div>
      ))}
    </div>
  );
}

function MediaGrid({
  caseDatabasePath,
  entries,
  keywords,
}: {
  caseDatabasePath: string | null;
  entries: AssistantMedia[];
  keywords: string[];
}) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-2">
      {entries.slice(0, MAX_ROWS).map(({ item, at }) => (
        <figure key={item.path} className="flex flex-col gap-1 overflow-hidden rounded-sm border" title={item.path}>
          <div className="aspect-[4/3] bg-muted">
            {item.mediaType === "video" ? (
              <MediaVideoPreview item={item} className="h-full w-full" />
            ) : (
              <MediaImagePreview
                caseDatabasePath={caseDatabasePath}
                item={item}
                className="h-full w-full object-cover"
              />
            )}
          </div>
          <figcaption className="flex flex-col gap-0.5 px-1.5 pb-1.5 text-[11px]">
            <span className="truncate font-medium">{item.name}</span>
            <span className="text-muted-foreground">{formatDate(at)}</span>
            <span className="line-clamp-2">
              {item.caption ? highlight(item.caption, keywords) : <em className="text-muted-foreground">Not tagged yet</em>}
            </span>
          </figcaption>
        </figure>
      ))}
    </div>
  );
}

function highlight(text: string, keywords: string[]) {
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

function participantName(value: string) {
  const { name, phone } = splitParticipant(value);
  return name || phone || value;
}

function formatDate(date: Date | null) {
  return date ? DATE_TIME_FORMAT.format(date) : "Unknown date";
}

function formatDuration(seconds: number | null) {
  if (seconds === null) {
    return "-";
  }
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

function summarize(results: AssistantResults) {
  const parts = results.searched
    .map((target) => ({ target, count: results[target].length }))
    .filter(({ count }) => count > 0)
    .map(({ target, count }) => `${count.toLocaleString()} ${TARGET_LABELS[target].toLowerCase()}`);

  return parts.length > 0 ? `Found ${parts.join(", ")}.` : "Nothing matched these filters.";
}

function buildExamples(index: AssistantIndex | null) {
  const examples: string[] = [];
  const counts = new Map<string, number>();

  for (const message of index?.messages ?? []) {
    for (const value of [message.sender, ...message.recipients]) {
      const { name } = splitParticipant(value);
      if (name) {
        counts.set(name, (counts.get(name) ?? 0) + 1);
      }
    }
  }

  const [first, second] = Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1])
    .map(([name]) => name.split(" ")[0]);
  const years = (index?.messages ?? []).map((message) => message.at?.getFullYear()).filter(Boolean);
  const latestYear = years.length > 0 ? Math.max(...(years as number[])) : null;

  examples.push(
    first && second
      ? `Show me the messages between ${first} and ${second}${latestYear ? ` March - Jun ${latestYear}` : ""}`
      : "Show me all messages",
  );
  if (second) {
    examples.push(`Messages from ${second}`);
  }
  examples.push('Messages containing "delete"', second ? `Calls with ${second}` : "Show me all calls");
  examples.push("Photos of cars", latestYear ? `Photos from July ${latestYear}` : "Photos from last month", "Contacts");

  return examples;
}
