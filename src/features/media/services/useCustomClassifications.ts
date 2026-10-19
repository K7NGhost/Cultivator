import { useEffect, useRef, useState } from "react";
import type { ClassificationDefinition } from "../model/vicClassification";
import { listCustomClassifications, saveCustomClassification, deleteCustomClassification } from "./mediaRepository";

export function useCustomClassifications(
  casePath: string | null,
  onChanged: (deletedCategory?: number) => void,
  onError: (error: string) => void,
) {
  const context = useRef({ path: casePath });
  if (context.current.path !== casePath) context.current = { path: casePath };
  const [state, setState] = useState<{ context: typeof context.current | null; categories: ClassificationDefinition[] }>({ context: null, categories: [] });
  const [busyContext, setBusyContext] = useState<typeof context.current | null>(null);
  useEffect(() => {
    let current = true;
    const requestContext = context.current;
    if (!casePath) { setState({ context: requestContext, categories: [] }); return; }
    void listCustomClassifications(casePath).then((categories) => {
      if (current) setState({ context: requestContext, categories });
    }).catch((error: unknown) => {
      if (current) { setState({ context: requestContext, categories: [] }); onError(String(error)); }
    });
    return () => { current = false; };
  }, [casePath, onError]);
  const ready = !!casePath && state.context === context.current;

  async function onSave(category: number | null, name: string, color: string) {
    if (!casePath || !ready) throw new Error("Open a case before adding classifications.");
    const requestContext = context.current;
    setBusyContext(requestContext);
    try {
      const saved = await saveCustomClassification(casePath, category, name, color);
      if (context.current !== requestContext) return;
      setState((current) => ({ context: requestContext, categories: category === null ? [...current.categories, saved]
        : current.categories.map((entry) => entry.value === saved.value ? saved : entry) }));
      onChanged();
    } finally { setBusyContext((current) => current === requestContext ? null : current); }
  }
  async function onDelete(category: number) {
    if (!casePath || !ready) throw new Error("Open a case before deleting classifications.");
    const requestContext = context.current;
    setBusyContext(requestContext);
    try {
      await deleteCustomClassification(casePath, category);
      if (context.current !== requestContext) return;
      setState((current) => ({ context: requestContext, categories: current.categories.filter((entry) => entry.value !== category) }));
      onChanged(category);
    } finally { setBusyContext((current) => current === requestContext ? null : current); }
  }
  return { categories: ready ? state.categories : [], ready, busy: !!casePath && busyContext === context.current, onSave, onDelete };
}
