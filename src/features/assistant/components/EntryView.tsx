import type { ReactNode } from "react";
import { Contact, Film, Image as ImageIcon, Info, LoaderCircle, MessageSquare, Phone, User } from "lucide-react";

import type { AssistantQuery } from "@/features/assistant/assistantQuery";
import { relatedOnly, strongOnly, summarize } from "@/features/assistant/assistantFormat";
import { CallList } from "@/features/assistant/components/CallList";
import { ContactList } from "@/features/assistant/components/ContactList";
import { FilterChips } from "@/features/assistant/components/FilterChips";
import { MediaGrid } from "@/features/assistant/components/MediaGrid";
import { MessageList } from "@/features/assistant/components/MessageList";
import { PossibleLinks } from "@/features/assistant/components/PossibleLinks";
import { ResultSection } from "@/features/assistant/components/ResultSection";
import type { AssistantEntry, ResultActions } from "@/features/assistant/types";
import { cn } from "@/lib/utils";

export function EntryView({
  actions,
  caseDatabasePath,
  entry,
  firstModelLoad,
  meaningAvailable,
  onSetUseMeaning,
  onSetUsePrecision,
  onUpdate,
}: {
  actions: ResultActions;
  caseDatabasePath: string | null;
  entry: AssistantEntry;
  firstModelLoad: boolean;
  meaningAvailable: boolean;
  onSetUseMeaning: (useMeaning: boolean) => void;
  onSetUsePrecision: (usePrecision: boolean) => void;
  onUpdate: (update: (query: AssistantQuery) => AssistantQuery) => void;
}) {
  const { query, results } = entry;
  const hasContent = query.keywords.length > 0;
  const showMatchBadges = hasContent;

  return (
    <article className="flex flex-col gap-2 rounded-sm border p-2">
      <div className="flex items-start gap-2 text-xs">
        <User className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <p className="font-medium">{query.text}</p>
      </div>

      <FilterChips
        entry={entry}
        onSetUseMeaning={onSetUseMeaning}
        onSetUsePrecision={onSetUsePrecision}
        onUpdate={onUpdate}
      />

      {entry.isThinking && (
        <p className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
          <LoaderCircle className="size-3 animate-spin" aria-hidden="true" />
          {firstModelLoad
            ? "Searching by meaning — loading the local AI model (first question can take ~20 s)..."
            : "Searching by meaning..."}
        </p>
      )}
      {hasContent && !meaningAvailable && (
        <Note>
          Content words are matched exactly. Run the Semantic Index (AI) plugin once to also find
          results by meaning (e.g. &ldquo;money&rdquo; finding transfers) and to handle typos.
        </Note>
      )}
      {entry.isChecking && (
        <p className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
          <LoaderCircle className="size-3 animate-spin" aria-hidden="true" />
          Double-checking meaning matches with the local precision model...
        </p>
      )}
      {entry.semanticError && <Note tone="error">Meaning search failed: {entry.semanticError}</Note>}
      {entry.checkError && (
        <Note tone="error">
          Precision check failed, so these meaning matches are unchecked: {entry.checkError}
        </Note>
      )}
      {entry.semantic && entry.semantic.unindexedCount > 0 && entry.useMeaning && (
        <Note>
          {entry.semantic.unindexedCount.toLocaleString()} messages or captions aren&apos;t in the
          meaning index yet (only exact matches for those). Run Semantic Index (AI) again to include
          them.
        </Note>
      )}
      {results.notes.map((note) => (
        <Note key={note}>{note}</Note>
      ))}

      <p className="text-xs text-muted-foreground">{summarize(results)}</p>

      {results.searched.includes("messages") && (
        <ResultSection icon={MessageSquare} title="Messages" items={strongOnly(results.messages)}>
          {(items) => (
            <MessageList
              actions={actions}
              messages={items}
              keywords={query.keywords}
              showMatchBadges={showMatchBadges}
            />
          )}
        </ResultSection>
      )}
      {results.searched.includes("calls") && (
        <ResultSection icon={Phone} title="Calls" items={results.calls}>
          {(items) => <CallList actions={actions} calls={items} />}
        </ResultSection>
      )}
      {results.searched.includes("contacts") && (
        <ResultSection icon={Contact} title="Contacts" items={results.contacts}>
          {(items) => <ContactList actions={actions} contacts={items} />}
        </ResultSection>
      )}
      {results.searched.includes("photos") && (
        <ResultSection icon={ImageIcon} title="Photos" items={strongOnly(results.photos)}>
          {(items) => (
            <MediaGrid
              actions={actions}
              caseDatabasePath={caseDatabasePath}
              entries={items}
              keywords={query.keywords}
              showMatchBadges={showMatchBadges}
            />
          )}
        </ResultSection>
      )}
      {results.searched.includes("videos") && (
        <ResultSection icon={Film} title="Videos" items={strongOnly(results.videos)}>
          {(items) => (
            <MediaGrid
              actions={actions}
              caseDatabasePath={caseDatabasePath}
              entries={items}
              keywords={query.keywords}
              showMatchBadges={showMatchBadges}
            />
          )}
        </ResultSection>
      )}

      <PossibleLinks
        actions={actions}
        caseDatabasePath={caseDatabasePath}
        keywords={query.keywords}
        messages={relatedOnly(results.messages)}
        photos={relatedOnly(results.photos)}
        videos={relatedOnly(results.videos)}
      />
    </article>
  );
}

function Note({ children, tone = "info" }: { children: ReactNode; tone?: "info" | "error" }) {
  return (
    <p
      className={cn(
        "flex items-start gap-1.5 text-[11px]",
        tone === "error" ? "text-destructive" : "text-muted-foreground",
      )}
    >
      <Info className="mt-0.5 size-3 shrink-0" aria-hidden="true" />
      <span>{children}</span>
    </p>
  );
}
