import { Brain, Check } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import type { AssistantMatch } from "@/features/assistant/assistantEngine";

export function MatchBadge({ match }: { match?: AssistantMatch }) {
  if (!match) {
    return null;
  }
  return match.reason === "exact" ? (
    <Badge variant="outline" className="h-4 rounded-sm px-1 text-[10px] font-normal">
      exact
    </Badge>
  ) : (
    <Badge
      variant="secondary"
      className="h-4 gap-0.5 rounded-sm px-1 text-[10px] font-normal"
      title={
        match.checked
          ? "Similarity from the local BGE-M3 model, confirmed by the local precision check (Qwen3-Reranker). Relative to this question; not a probability."
          : "Similarity from the local BGE-M3 model. Relative to this question; not a probability."
      }
    >
      <Brain className="size-2.5" aria-hidden="true" />
      meaning {match.score?.toFixed(2)}
      {match.checked && <Check className="size-2.5" aria-label="double-checked" />}
    </Badge>
  );
}
