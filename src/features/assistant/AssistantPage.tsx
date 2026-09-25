import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useLocation } from "react-router-dom";
import { AlertCircle, RefreshCw, SendHorizontal, Sparkles } from "lucide-react";

import { useReveal } from "@/app/reveal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { listArtifacts } from "@/features/artifacts/artifactRepository";
import {
  indexAssistantData,
  runAssistantQuery,
  semanticKey,
  type AssistantIndex,
  type AssistantMessage,
} from "@/features/assistant/assistantEngine";
import { buildExamples } from "@/features/assistant/assistantExamples";
import { parseAssistantQuery, type AssistantQuery } from "@/features/assistant/assistantQuery";
import { ConversationDialog } from "@/features/assistant/components/ConversationDialog";
import { EmptyState } from "@/features/assistant/components/EmptyState";
import { EntryView } from "@/features/assistant/components/EntryView";
import type { AssistantEntry, SemanticState } from "@/features/assistant/types";
import { useCases } from "@/features/cases/case-provider";
import { listMediaGallery } from "@/features/media/mediaRepository";
import { getSemanticModelStatus, semanticSearch } from "@/features/plugins/pluginRepository";
import { cn } from "@/lib/utils";

let nextEntryId = 0;

export function AssistantPage() {
  const { activeCase } = useCases();
  const location = useLocation();
  const isActive = location.pathname === "/assistant";
  const [index, setIndex] = useState<AssistantIndex | null>(null);
  const [meaningAvailable, setMeaningAvailable] = useState(false);
  const [hasAskedModel, setHasAskedModel] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [input, setInput] = useState("");
  const [entries, setEntries] = useState<AssistantEntry[]>([]);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const indexRef = useRef<AssistantIndex | null>(null);
  const reveal = useReveal();
  const [openConversation, setOpenConversation] = useState<{
    anchor: AssistantMessage;
    keywords: string[];
  } | null>(null);

  // Reload whenever the page is shown, so results reflect plugins that
  // finished while the examiner was elsewhere.
  useEffect(() => {
    if (!activeCase) {
      setIndex(null);
      indexRef.current = null;
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
      getSemanticModelStatus().catch(() => null),
    ])
      .then(([artifacts, gallery, modelStatus]) => {
        if (!isCurrent) {
          return;
        }
        const nextIndex = indexAssistantData(artifacts, [...gallery.photos, ...gallery.videos]);
        indexRef.current = nextIndex;
        setIndex(nextIndex);
        setMeaningAvailable(Boolean(modelStatus?.packagesInstalled && modelStatus.modelCached));
        setEntries((current) =>
          current.map((entry) => ({
            ...entry,
            results: runAssistantQuery(entry.query, nextIndex, entry.useMeaning ? entry.semantic?.scores : null),
          })),
        );
      })
      .catch((caughtError) => {
        if (isCurrent) {
          setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
          setIndex(null);
          indexRef.current = null;
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

  function patchEntry(entryId: string, patch: (entry: AssistantEntry) => AssistantEntry) {
    setEntries((current) => current.map((entry) => (entry.id === entryId ? patch(entry) : entry)));
  }

  /** Asks the local model which items match the question's content words. */
  function fetchMeaning(entryId: string, query: AssistantQuery) {
    if (!activeCase || !meaningAvailable || query.keywords.length === 0) {
      return;
    }

    patchEntry(entryId, (entry) => ({ ...entry, isThinking: true, semanticError: null }));

    semanticSearch(activeCase.databasePath, query.keywords.join(" "))
      .then((result) => {
        const semantic: SemanticState = {
          scores: new Map(
            result.scores.map((score) => [
              semanticKey(score.refKind, score.refId),
              { score: score.score, tier: score.tier },
            ]),
          ),
          corrections: result.corrections,
          unindexedCount: result.unindexedCount,
        };
        patchEntry(entryId, (entry) => {
          const latestIndex = indexRef.current;
          // Ignore replies for a filter the examiner has since changed.
          if (entry.query !== query || !latestIndex) {
            return entry;
          }
          return {
            ...entry,
            semantic,
            isThinking: false,
            results: runAssistantQuery(query, latestIndex, entry.useMeaning ? semantic.scores : null),
          };
        });
      })
      .catch((caughtError) => {
        patchEntry(entryId, (entry) => ({
          ...entry,
          isThinking: false,
          semanticError: caughtError instanceof Error ? caughtError.message : String(caughtError),
        }));
      })
      .finally(() => setHasAskedModel(true));
  }

  function ask(text: string) {
    if (!index || !text.trim()) {
      return;
    }
    const query = parseAssistantQuery(text.trim(), { people: index.people });
    const id = `entry-${nextEntryId++}`;
    setEntries((current) => [
      ...current,
      {
        id,
        query,
        results: runAssistantQuery(query, index),
        useMeaning: true,
        semantic: null,
        isThinking: false,
        semanticError: null,
      },
    ]);
    setInput("");
    fetchMeaning(id, query);
  }

  function updateQuery(entryId: string, update: (query: AssistantQuery) => AssistantQuery) {
    if (!index) {
      return;
    }
    const entry = entries.find((candidate) => candidate.id === entryId);
    if (!entry) {
      return;
    }
    const query = { ...update(entry.query), notes: [] };
    const keywordsChanged = query.keywords.join("\u0000") !== entry.query.keywords.join("\u0000");
    const semantic = keywordsChanged ? null : entry.semantic;

    patchEntry(entryId, (current) => ({
      ...current,
      query,
      semantic,
      results: runAssistantQuery(query, index, current.useMeaning ? semantic?.scores : null),
    }));
    if (keywordsChanged && entry.useMeaning) {
      fetchMeaning(entryId, query);
    }
  }

  function setUseMeaning(entryId: string, useMeaning: boolean) {
    if (!index) {
      return;
    }
    patchEntry(entryId, (entry) => ({
      ...entry,
      useMeaning,
      results: runAssistantQuery(entry.query, index, useMeaning ? entry.semantic?.scores : null),
    }));
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
        <Badge
          variant="outline"
          className="h-5 rounded-sm text-[11px]"
          title={
            meaningAvailable
              ? "Content words are matched by meaning with the on-device BGE-M3 model."
              : "Content words are matched exactly. Run Semantic Index (AI) to search by meaning."
          }
        >
          {meaningAvailable ? "Offline · BGE-M3 meaning search" : "Offline · exact keywords"}
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
              meaningAvailable={meaningAvailable}
              onAsk={ask}
            />
          ) : (
            entries.map((entry) => (
              <EntryView
                key={entry.id}
                actions={{
                  openMessage: (message) =>
                    setOpenConversation({ anchor: message, keywords: entry.query.keywords }),
                  reveal,
                }}
                caseDatabasePath={activeCase?.databasePath ?? null}
                entry={entry}
                firstModelLoad={!hasAskedModel}
                meaningAvailable={meaningAvailable}
                onSetUseMeaning={(useMeaning) => setUseMeaning(entry.id, useMeaning)}
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
                ? 'Ask about this case, e.g. "messages about money between John and Jane March - Jun 2025"'
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

      <ConversationDialog
        anchor={openConversation?.anchor ?? null}
        keywords={openConversation?.keywords ?? []}
        messages={index?.messages ?? []}
        onClose={() => setOpenConversation(null)}
        onReveal={(target) => {
          setOpenConversation(null);
          reveal(target);
        }}
      />
    </div>
  );
}
