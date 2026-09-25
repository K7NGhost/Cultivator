import type { AssistantMessage, Matched } from "@/features/assistant/assistantEngine";
import { clickableProps, formatDate, highlight, participantName } from "@/features/assistant/assistantFormat";
import { MatchBadge } from "@/features/assistant/components/MatchBadge";
import type { ResultActions } from "@/features/assistant/types";

export function MessageList({
  actions,
  messages,
  keywords,
  showMatchBadges,
}: {
  actions: ResultActions;
  messages: Matched<AssistantMessage>[];
  keywords: string[];
  showMatchBadges: boolean;
}) {
  return (
    <div className="rounded-sm border">
      {messages.map((message) => (
        <div
          key={message.id}
          className="grid cursor-pointer grid-cols-[9.5rem_minmax(0,15rem)_minmax(0,1fr)] gap-2 border-b px-2 py-1 text-xs last:border-b-0 hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
          title="Open this conversation"
          {...clickableProps(() => actions.openMessage(message), "Open conversation")}
        >
          <span className="font-mono text-[11px] text-muted-foreground">{formatDate(message.at)}</span>
          <span className="truncate" title={`${message.sender} → ${message.recipients.join(", ")}`}>
            <span className="font-medium">{participantName(message.sender)}</span>
            <span className="text-muted-foreground"> → </span>
            {message.recipients.map(participantName).join(", ")}
          </span>
          <span className="flex min-w-0 items-start justify-between gap-2">
            <span className="whitespace-pre-wrap break-words">{highlight(message.body, keywords)}</span>
            {showMatchBadges && <MatchBadge match={message.match} />}
          </span>
        </div>
      ))}
    </div>
  );
}
