import type { StoredArtifactRecord } from "../model/types";

/** Preserve source records, with explorer counts limited to tagged occurrences. */
export function filterTaggedArtifacts(artifacts: StoredArtifactRecord[], taggedOnly: boolean, counts: ReadonlyMap<string, number>): StoredArtifactRecord[] {
  if (!taggedOnly) return artifacts;
  return artifacts.flatMap((artifact) => {
    const count = counts.get(artifact.id) ?? 0;
    return count > 0 ? [{ ...artifact, summaryOnly: true, entryCount: count }] : [];
  });
}
