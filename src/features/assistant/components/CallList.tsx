import type { AssistantCall } from "@/features/assistant/assistantEngine";
import { clickableProps, formatDate, formatDuration } from "@/features/assistant/assistantFormat";
import type { ResultActions } from "@/features/assistant/types";

export function CallList({ actions, calls }: { actions: ResultActions; calls: AssistantCall[] }) {
  return (
    <div className="rounded-sm border">
      {calls.map((call) => (
        <div
          key={call.id}
          className="grid cursor-pointer grid-cols-[9.5rem_6rem_minmax(0,1fr)_5rem] gap-2 border-b px-2 py-1 text-xs last:border-b-0 hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
          title="Open in Artifacts"
          {...clickableProps(
            () => actions.reveal({ kind: "artifact", artifactId: call.id }),
            "Open call in Artifacts",
          )}
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
