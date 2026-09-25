import { useState } from "react";
import { ChevronDown, ChevronRight, Link2 } from "lucide-react";

import type { AssistantMedia, AssistantMessage, Matched } from "@/features/assistant/assistantEngine";
import { MAX_ROWS } from "@/features/assistant/assistantFormat";
import { MediaGrid } from "@/features/assistant/components/MediaGrid";
import { MessageList } from "@/features/assistant/components/MessageList";
import type { ResultActions } from "@/features/assistant/types";

/**
 * Weaker meaning matches, kept out of the answer and collapsed into one
 * dropdown so they never crowd out the confident results.
 */
export function PossibleLinks({
  actions,
  caseDatabasePath,
  keywords,
  messages,
  photos,
  videos,
}: {
  actions: ResultActions;
  caseDatabasePath: string | null;
  keywords: string[];
  messages: Matched<AssistantMessage>[];
  photos: Matched<AssistantMedia>[];
  videos: Matched<AssistantMedia>[];
}) {
  const [isOpen, setIsOpen] = useState(false);
  const total = messages.length + photos.length + videos.length;

  if (total === 0) {
    return null;
  }

  const groups = [
    { label: "messages", count: messages.length },
    { label: "photos", count: photos.length },
    { label: "videos", count: videos.length },
  ].filter((group) => group.count > 0);

  return (
    <section className="rounded-sm border border-dashed">
      <button
        type="button"
        className="flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-expanded={isOpen}
        onClick={() => setIsOpen((current) => !current)}
      >
        {isOpen ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        <Link2 className="size-3.5" aria-hidden="true" />
        <span className="font-medium">Possible links</span>
        <span>
          · {total.toLocaleString()} weaker matches (
          {groups.map((group) => `${group.count} ${group.label}`).join(", ")})
        </span>
        <span className="ml-auto text-[10px]">may not be relevant</span>
      </button>
      {isOpen && (
        <div className="flex flex-col gap-2 border-t p-2 opacity-90">
          {messages.length > 0 && (
            <MessageList actions={actions} messages={messages.slice(0, MAX_ROWS)} keywords={keywords} showMatchBadges />
          )}
          {[...photos, ...videos].length > 0 && (
            <MediaGrid
              actions={actions}
              caseDatabasePath={caseDatabasePath}
              entries={[...photos, ...videos].slice(0, MAX_ROWS)}
              keywords={keywords}
              showMatchBadges
            />
          )}
        </div>
      )}
    </section>
  );
}
