import type { RevealTarget } from "@/app/reveal";
import type { AssistantMessage, AssistantResults, SemanticScores } from "@/features/assistant/assistantEngine";
import type { AssistantQuery } from "@/features/assistant/assistantQuery";
import type { SemanticSearchResult } from "@/features/plugins/pluginRepository";

export type SemanticState = {
  scores: SemanticScores;
  corrections: SemanticSearchResult["corrections"];
  unindexedCount: number;
};

export type AssistantEntry = {
  id: string;
  query: AssistantQuery;
  results: AssistantResults;
  useMeaning: boolean;
  semantic: SemanticState | null;
  isThinking: boolean;
  semanticError: string | null;
};

/** What a clicked result can do: open its conversation or jump to another page. */
export type ResultActions = {
  openMessage: (message: AssistantMessage) => void;
  reveal: (target: RevealTarget) => void;
};
