/** A `[start, end)` character range in a question being parsed. */
export type Span = [number, number];

/**
 * Replaces each span with spaces, so later parsing steps skip words an
 * earlier step already consumed while every other index stays valid.
 */
export function blankSpans(text: string, spans: Span[]) {
  let result = text;
  for (const [start, end] of spans) {
    result = result.slice(0, start) + " ".repeat(end - start) + result.slice(end);
  }
  return result;
}
