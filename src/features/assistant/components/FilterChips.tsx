import { Brain, Calendar, Sparkles, Tag, User, X } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import type { AssistantQuery } from "@/features/assistant/assistantQuery";
import { TARGET_LABELS } from "@/features/assistant/assistantFormat";
import type { AssistantEntry } from "@/features/assistant/types";

type PersonRole = "with" | "from" | "to";

const ROLE_LABELS: Record<PersonRole, string> = { with: "With", from: "From", to: "To" };

export function FilterChips({
  entry,
  onSetUseMeaning,
  onUpdate,
}: {
  entry: AssistantEntry;
  onSetUseMeaning: (useMeaning: boolean) => void;
  onUpdate: (update: (query: AssistantQuery) => AssistantQuery) => void;
}) {
  const { query } = entry;
  const people = (["with", "from", "to"] as const).flatMap((role) =>
    query.people[role].map((person, index) => ({ role, person, index })),
  );
  const corrections = entry.semantic?.corrections ?? [];

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
      {entry.useMeaning && entry.semantic && query.keywords.length > 0 && (
        <Chip
          icon={Brain}
          label={
            corrections.length > 0
              ? `Meaning search (spelling: ${corrections.map((c) => `${c.from} → ${c.to}`).join(", ")})`
              : "Meaning search"
          }
          title="Also matching by meaning with the local BGE-M3 model. Remove to match exact words only."
          onRemove={() => onSetUseMeaning(false)}
        />
      )}
      {!entry.useMeaning && entry.semantic && (
        <button
          type="button"
          className="rounded-sm px-1 text-[11px] text-muted-foreground underline-offset-2 hover:underline"
          onClick={() => onSetUseMeaning(true)}
        >
          Turn meaning search back on
        </button>
      )}
    </div>
  );
}

function Chip({
  icon: Icon,
  label,
  onRemove,
  title,
}: {
  icon: typeof Tag;
  label: string;
  onRemove?: () => void;
  title?: string;
}) {
  return (
    <Badge
      variant="secondary"
      className="h-5 gap-1 rounded-sm px-1.5 text-[11px] font-normal"
      title={title}
    >
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
