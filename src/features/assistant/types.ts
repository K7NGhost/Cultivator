import type { RevealTarget } from "@/app/reveal";
import type { AssistantMessage, AssistantResults, SemanticScores } from "@/features/assistant/assistantEngine";
import type { AssistantQuery } from "@/features/assistant/assistantQuery";
import type { SemanticSearchResult } from "@/features/plugins/pluginRepository";

export type SemanticState = {
  scores: SemanticScores;
  corrections: SemanticSearchResult["corrections"];
  unindexedCount: number;
  /** The precision check confirmed these scores. */
  checked: boolean;
};

export type AssistantEntry = {
  id: string;
  query: AssistantQuery;
  results: AssistantResults;
  useMeaning: boolean;
  /** Meaning matches as the vector search found them. */
  semantic: SemanticState | null;
  /** The same matches after the precision check, once it has run. */
  checkedSemantic: SemanticState | null;
  /** Show the checked matches; off shows everything the vector search found. */
  usePrecision: boolean;
  isThinking: boolean;
  semanticError: string | null;
  /** The precision check is double-checking the meaning matches. */
  isChecking: boolean;
  checkError: string | null;
};

/** What a clicked result can do: open its conversation or jump to another page. */
export type ResultActions = {
  openMessage: (message: AssistantMessage) => void;
  reveal: (target: RevealTarget) => void;
};
