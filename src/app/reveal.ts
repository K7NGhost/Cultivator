import { useEffect, useRef } from "react";
import { useLocation, useNavigate } from "react-router-dom";

/** Something another page can be asked to open and select. */
export type RevealTarget =
  | { kind: "file"; path: string }
  | { kind: "media"; path: string; mediaType: "image" | "video" }
  | { kind: "artifact"; artifactId: string };

type RevealState = { reveal?: RevealTarget };

const REVEAL_PATHS: Record<RevealTarget["kind"], string> = {
  file: "/files",
  media: "/media",
  artifact: "/artifacts",
};

/** Navigates to the page that owns `target` and asks it to select it. */
export function useReveal() {
  const navigate = useNavigate();

  return (target: RevealTarget) => {
    navigate(REVEAL_PATHS[target.kind], { state: { reveal: target } satisfies RevealState });
  };
}

/**
 * Calls `onReveal` once per navigation that carries a reveal request for this
 * page. Workspace pages stay mounted, so this keys off the location, not mount.
 */
export function useRevealRequest<K extends RevealTarget["kind"]>(
  kind: K,
  onReveal: (target: Extract<RevealTarget, { kind: K }>) => void,
) {
  const location = useLocation();
  const handledKeyRef = useRef<string | null>(null);
  const onRevealRef = useRef(onReveal);
  onRevealRef.current = onReveal;

  useEffect(() => {
    const reveal = (location.state as RevealState | null)?.reveal;

    if (!reveal || reveal.kind !== kind || handledKeyRef.current === location.key) {
      return;
    }

    handledKeyRef.current = location.key;
    onRevealRef.current(reveal as Extract<RevealTarget, { kind: K }>);
  }, [kind, location.key, location.state]);
}

/** Case-insensitive, separator-agnostic path comparison (Windows evidence paths). */
export function normalizeRevealPath(path: string) {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

export function isSameOrInsidePath(path: string, root: string) {
  const normalizedPath = normalizeRevealPath(path);
  const normalizedRoot = normalizeRevealPath(root);

  return normalizedPath === normalizedRoot || normalizedPath.startsWith(`${normalizedRoot}/`);
}
