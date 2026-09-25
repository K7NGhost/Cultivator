import { FolderOpen } from "lucide-react";

import type { AssistantMedia, Matched } from "@/features/assistant/assistantEngine";
import { clickableProps, formatDate, highlight } from "@/features/assistant/assistantFormat";
import { MatchBadge } from "@/features/assistant/components/MatchBadge";
import type { ResultActions } from "@/features/assistant/types";
import { MediaImagePreview } from "@/features/media/components/MediaImagePreview";
import { MediaVideoPreview } from "@/features/media/components/MediaVideoPreview";

export function MediaGrid({
  actions,
  caseDatabasePath,
  entries,
  keywords,
  showMatchBadges,
}: {
  actions: ResultActions;
  caseDatabasePath: string | null;
  entries: Matched<AssistantMedia>[];
  keywords: string[];
  showMatchBadges: boolean;
}) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-2">
      {entries.map(({ item, at, match }) => (
        <figure key={item.path} className="flex flex-col gap-1 overflow-hidden rounded-sm border" title={item.path}>
          <div
            className="aspect-[4/3] cursor-pointer bg-muted outline-none hover:opacity-90 focus-visible:ring-2 focus-visible:ring-ring"
            title={`Open in Media: ${item.path}`}
            {...clickableProps(
              () => actions.reveal({ kind: "media", path: item.path, mediaType: item.mediaType }),
              `Open ${item.name} in Media`,
            )}
          >
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
            <span className="flex items-center justify-between gap-1">
              <span className="truncate font-medium">{item.name}</span>
              {showMatchBadges && <MatchBadge match={match} />}
              <button
                type="button"
                className="ml-auto shrink-0 rounded-sm p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"
                title="Show file in Files"
                aria-label={`Show ${item.name} in Files`}
                onClick={() => actions.reveal({ kind: "file", path: item.path })}
              >
                <FolderOpen className="size-3" />
              </button>
            </span>
            <span className="text-muted-foreground">{formatDate(at)}</span>
            <span className="line-clamp-2">
              {item.caption ? (
                highlight(item.caption, keywords)
              ) : (
                <em className="text-muted-foreground">Not tagged yet</em>
              )}
            </span>
          </figcaption>
        </figure>
      ))}
    </div>
  );
}
