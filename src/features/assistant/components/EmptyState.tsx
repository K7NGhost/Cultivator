import { Sparkles } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { AssistantIndex } from "@/features/assistant/assistantEngine";

export function EmptyState({
  examples,
  hasCase,
  index,
  meaningAvailable,
  onAsk,
}: {
  examples: string[];
  hasCase: boolean;
  index: AssistantIndex | null;
  meaningAvailable: boolean;
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
        Questions are turned into a filter over messages, calls, contacts, and photos. People and
        dates are read from your question; content words are matched
        {meaningAvailable
          ? " by meaning with the local BGE-M3 model, so “money” also finds transfers and cash, and typos are corrected."
          : " exactly. Run the Semantic Index (AI) plugin to match them by meaning instead."}{" "}
        The filter is shown with every answer so you can check and edit it. Everything runs on this
        machine.
      </p>
      {!hasCase && <p className="text-muted-foreground">Open or create a case to start.</p>}
      {hasCase && index && !hasData && (
        <p className="rounded-sm border px-2 py-1.5 text-muted-foreground">
          No searchable data yet. Run plugins on a datasource first: a message parser (e.g. JSON
          Chat Export) for messages, calls, and contacts; Image Metadata + Media Tagging (AI) for
          photos; then Semantic Index (AI) for meaning search.
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
