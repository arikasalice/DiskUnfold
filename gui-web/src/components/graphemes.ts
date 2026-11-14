/**
 * Splits text into user-perceived characters (extended grapheme clusters), so
 * a base letter and its combining marks, a conjoining Hangul jamo sequence, a
 * ZWJ emoji or a surrogate pair is never cut in the middle. Without
 * `Intl.Segmenter` it falls back to code points.
 */
export function graphemes(text: string): string[] {
  if (typeof Intl !== "undefined" && typeof Intl.Segmenter === "function") {
    const segmenter = new Intl.Segmenter("und", { granularity: "grapheme" });
    return Array.from(segmenter.segment(text), (part) => part.segment);
  }
  return Array.from(text);
}
