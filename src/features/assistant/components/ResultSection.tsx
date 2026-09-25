import type { ReactNode } from "react";
import type { Tag } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { MAX_ROWS } from "@/features/assistant/assistantFormat";

/** One result type's confident matches; weaker ones live in Possible links. */
export function ResultSection<T extends object>({
  children,
  icon: Icon,
  items,
  title,
}: {
  children: (items: T[]) => ReactNode;
  icon: typeof Tag;
  items: T[];
  title: string;
}) {
  return (
    <section className="flex flex-col gap-1">
      <div className="flex items-center gap-1.5 text-xs font-medium">
        <Icon className="size-3.5 text-muted-foreground" aria-hidden="true" />
        {title}
        <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px]">
          {items.length.toLocaleString()}
        </Badge>
        {items.length > MAX_ROWS && (
          <span className="text-[11px] font-normal text-muted-foreground">showing first {MAX_ROWS}</span>
        )}
      </div>
      {items.length === 0 ? (
        <p className="text-[11px] text-muted-foreground">No {title.toLowerCase()} match.</p>
      ) : (
        children(items.slice(0, MAX_ROWS))
      )}
    </section>
  );
}
