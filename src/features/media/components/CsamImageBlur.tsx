import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { Eye, EyeOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { MediaItem } from "../model/types";
import { isCsamImage } from "../model/vicClassification";

export const CSAM_IMAGE_BLUR_FILTER = "blur(32px)";

type RevealState = {
  paths: ReadonlySet<string>;
  toggle: (path: string) => void;
  hide: (path: string) => void;
};
const RevealContext = createContext<RevealState | null>(null);

export function CsamImageRevealProvider({ children }: { children: ReactNode }) {
  const [paths, setPaths] = useState<ReadonlySet<string>>(() => new Set());
  const toggle = useCallback((path: string) => setPaths((current) => {
    const next = new Set(current);
    if (next.has(path)) next.delete(path); else next.add(path);
    return next;
  }), []);
  const hide = useCallback((path: string) => setPaths((current) => {
    if (!current.has(path)) return current;
    const next = new Set(current); next.delete(path); return next;
  }), []);
  return <RevealContext.Provider value={{ paths, toggle, hide }}>{children}</RevealContext.Provider>;
}

export function useCsamImageReveal(item: MediaItem | null) {
  const state = useContext(RevealContext);
  if (!state) throw new Error("CSAM image controls require a media reveal provider.");
  // Evidence paths identify the same image across gallery, details and viewer
  // records, even when those records have different IDs or resolved media paths.
  const path = item?.path.replace(/\\/g, "/") ?? "";
  const classified = !!item && isCsamImage(item);
  useEffect(() => { if (path && !classified) state.hide(path); }, [path, classified, state.hide]);
  const revealed = classified && state.paths.has(path);
  return { classified, revealed, blurred: classified && !revealed,
    toggle: () => { if (path) state.toggle(path); } };
}

export function CsamImageBlurToggle({ revealed, onToggle }: { revealed: boolean; onToggle: () => void }) {
  return <Button type="button" size="sm" variant="outline" className="h-7 text-xs" aria-pressed={revealed}
    onClick={onToggle}>{revealed ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}{revealed ? "Blur image" : "Unblur image"}</Button>;
}

export function CsamImageBlurIndicator() {
  return <span className="pointer-events-none absolute inset-0 flex items-center justify-center" aria-label="CSAM image blurred">
    <span className="rounded-sm bg-black/80 px-2 py-1 text-[11px] font-medium text-white">CSAM · blurred</span>
  </span>;
}
