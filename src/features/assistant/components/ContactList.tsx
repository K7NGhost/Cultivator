import type { AssistantContact } from "@/features/assistant/assistantEngine";
import { clickableProps } from "@/features/assistant/assistantFormat";
import type { ResultActions } from "@/features/assistant/types";

export function ContactList({ actions, contacts }: { actions: ResultActions; contacts: AssistantContact[] }) {
  return (
    <div className="rounded-sm border">
      {contacts.map((contact) => (
        <div
          key={contact.id}
          className="grid cursor-pointer grid-cols-[minmax(0,12rem)_minmax(0,10rem)_minmax(0,1fr)] gap-2 border-b px-2 py-1 text-xs last:border-b-0 hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
          title="Open in Artifacts"
          {...clickableProps(
            () => actions.reveal({ kind: "artifact", artifactId: contact.id }),
            "Open contact in Artifacts",
          )}
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
