import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import {
  createCase as persistCase,
  listCases,
} from "@/features/cases/caseRepository";
import type { CaseRecord, CreateCaseInput } from "@/features/cases/types";

const ACTIVE_CASE_STORAGE_KEY = "cultivator.activeCaseId";

type CaseContextValue = {
  activeCase: CaseRecord | null;
  cases: CaseRecord[];
  createCase: (input: CreateCaseInput) => Promise<CaseRecord>;
  error: string | null;
  isLoading: boolean;
  refreshCases: () => Promise<void>;
  selectCase: (caseId: string) => void;
};

const CaseContext = createContext<CaseContextValue | null>(null);

function loadActiveCaseId() {
  if (typeof localStorage === "undefined") {
    return null;
  }

  return localStorage.getItem(ACTIVE_CASE_STORAGE_KEY);
}

function saveActiveCaseId(caseId: string | null) {
  if (typeof localStorage === "undefined") {
    return;
  }

  if (!caseId) {
    localStorage.removeItem(ACTIVE_CASE_STORAGE_KEY);
    return;
  }

  localStorage.setItem(ACTIVE_CASE_STORAGE_KEY, caseId);
}

function getErrorMessage(caughtError: unknown) {
  return caughtError instanceof Error ? caughtError.message : String(caughtError);
}

export function CaseProvider({ children }: { children: ReactNode }) {
  const [cases, setCases] = useState<CaseRecord[]>([]);
  const [activeCaseId, setActiveCaseId] = useState<string | null>(
    loadActiveCaseId,
  );
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const activeCase = useMemo(() => {
    return cases.find((caseRecord) => caseRecord.id === activeCaseId) ?? null;
  }, [activeCaseId, cases]);

  const refreshCases = useCallback(async () => {
    setIsLoading(true);
    setError(null);

    try {
      const nextCases = await listCases();
      const savedActiveCaseId = loadActiveCaseId();
      const nextActiveCase =
        nextCases.find((caseRecord) => caseRecord.id === savedActiveCaseId) ??
        nextCases[0] ??
        null;

      setCases(nextCases);
      setActiveCaseId(nextActiveCase?.id ?? null);
      saveActiveCaseId(nextActiveCase?.id ?? null);
    } catch (caughtError) {
      setError(getErrorMessage(caughtError));
    } finally {
      setIsLoading(false);
    }
  }, []);

  const createCase = useCallback(async (input: CreateCaseInput) => {
    setError(null);

    try {
      const nextCase = await persistCase(input);

      setCases((currentCases) => [nextCase, ...currentCases]);
      setActiveCaseId(nextCase.id);
      saveActiveCaseId(nextCase.id);

      return nextCase;
    } catch (caughtError) {
      const message = getErrorMessage(caughtError);

      setError(message);
      throw new Error(message);
    }
  }, []);

  const selectCase = useCallback(
    (caseId: string) => {
      const nextActiveCase = cases.find((caseRecord) => caseRecord.id === caseId);

      setActiveCaseId(nextActiveCase?.id ?? null);
      saveActiveCaseId(nextActiveCase?.id ?? null);
    },
    [cases],
  );

  useEffect(() => {
    void refreshCases();
  }, [refreshCases]);

  const value = useMemo(
    () => ({
      activeCase,
      cases,
      createCase,
      error,
      isLoading,
      refreshCases,
      selectCase,
    }),
    [
      activeCase,
      cases,
      createCase,
      error,
      isLoading,
      refreshCases,
      selectCase,
    ],
  );

  return <CaseContext.Provider value={value}>{children}</CaseContext.Provider>;
}

export function useCases() {
  const context = useContext(CaseContext);

  if (!context) {
    throw new Error("useCases must be used within CaseProvider");
  }

  return context;
}
