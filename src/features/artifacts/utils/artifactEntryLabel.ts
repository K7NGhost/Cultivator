export function formatEntryHitLabel(
  entryCount: number,
  occurrenceCount: number,
  fileHits: number,
) {
  const entryLabel = entryCount === 1 ? "entry" : "entries";
  const occurrenceLabel = occurrenceCount === 1 ? "occurrence" : "occurrences";
  const fileLabel = fileHits === 1 ? "file hit" : "file hits";
  const entryText = `${entryCount.toLocaleString()} ${entryLabel}`;
  const fileText = `${fileHits.toLocaleString()} ${fileLabel}`;

  return occurrenceCount === entryCount
    ? `${entryText} / ${fileText}`
    : `${entryText} / ${occurrenceCount.toLocaleString()} ${occurrenceLabel} / ${fileText}`;
}

