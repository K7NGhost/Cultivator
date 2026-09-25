import { useEffect, useMemo, useRef } from "react";
import { Database, FolderOpen, MessageSquare } from "lucide-react";

import type { RevealTarget } from "@/app/reveal";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { conversationOf, type AssistantMessage } from "@/features/assistant/assistantEngine";
import { formatDate, highlight, participantName } from "@/features/assistant/assistantFormat";
import { cn } from "@/lib/utils";

/**
 * The whole conversation a message belongs to, scrolled to that message, with
 * links to its artifact and the evidence file it was parsed from.
 */
export function ConversationDialog({
  anchor,
  keywords,
  messages,
  onClose,
  onReveal,
}: {
  anchor: AssistantMessage | null;
  keywords: string[];
  messages: AssistantMessage[];
  onClose: () => void;
  onReveal: (target: RevealTarget) => void;
}) {
  const thread = useMemo(() => (anchor ? conversationOf(anchor, messages) : []), [anchor, messages]);
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const participants = anchor
    ? Array.from(new Set([anchor.sender, ...anchor.recipients].map(participantName)))
    : [];

  useEffect(() => {
    if (!anchor) {
      return;
    }
    // Wait for the dialog to lay out before scrolling the hit into view.
    const frame = requestAnimationFrame(() => anchorRef.current?.scrollIntoView({ block: "center" }));
    return () => cancelAnimationFrame(frame);
  }, [anchor]);

  return (
    <Dialog open={Boolean(anchor)} onOpenChange={(isOpen) => !isOpen && onClose()}>
      <DialogContent className="flex max-h-[calc(100vh-4rem)] max-w-3xl flex-col gap-0 rounded-sm p-0 sm:max-w-3xl">
        <DialogHeader className="border-b px-3 py-2">
          <DialogTitle className="flex items-center gap-1.5 text-sm">
            <MessageSquare className="size-3.5" aria-hidden="true" />
            Conversation: {participants.join(", ")}
          </DialogTitle>
          <DialogDescription className="text-xs">
            {thread.length.toLocaleString()} messages between these people. The one you clicked is
            highlighted.
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-auto p-3">
          <div className="flex flex-col gap-1.5">
            {thread.map((message) => {
              const isAnchor = message.id === anchor?.id;
              const isFromFirst = participantName(message.sender) === participants[0];

              return (
                <div
                  key={message.id}
                  ref={isAnchor ? anchorRef : undefined}
                  className={cn("flex", isFromFirst ? "justify-start" : "justify-end")}
                >
                  <div
                    className={cn(
                      "max-w-[75%] rounded-md border px-2 py-1 text-xs",
                      isFromFirst ? "bg-muted/50" : "bg-primary/10",
                      isAnchor && "ring-2 ring-yellow-400 dark:ring-yellow-500",
                    )}
                  >
                    <div className="mb-0.5 flex items-center justify-between gap-3 text-[10px] text-muted-foreground">
                      <span className="font-medium">{participantName(message.sender)}</span>
                      <span className="font-mono">{formatDate(message.at)}</span>
                    </div>
                    <div className="whitespace-pre-wrap break-words">
                      {isAnchor ? highlight(message.body, keywords) : message.body}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
        <DialogFooter className="border-t p-2">
          <span
            className="mr-auto self-center truncate font-mono text-[10px] text-muted-foreground"
            title={anchor?.filePath}
          >
            {anchor?.filePath}
          </span>
          <Button
            type="button"
            variant="outline"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={!anchor?.filePath}
            onClick={() => anchor && onReveal({ kind: "file", path: anchor.filePath })}
          >
            <FolderOpen className="size-3.5" aria-hidden="true" />
            Show source file
          </Button>
          <Button
            type="button"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={!anchor}
            onClick={() => anchor && onReveal({ kind: "artifact", artifactId: anchor.id })}
          >
            <Database className="size-3.5" aria-hidden="true" />
            Open in Artifacts
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
