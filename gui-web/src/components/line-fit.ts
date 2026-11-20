// Fits a path on one line by whole characters for MiddleEllipsisText (F02 README #170).
import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";
import { graphemes } from "./graphemes";

export function isSeparator(unit: string | undefined): boolean {
  return unit === "/" || unit === "\\";
}

/**
 * Splits the tail of a middle ellipsis into the part that may shrink and the
 * end that stays, so a last component wider than the line keeps its extension
 * (or its last characters) and loses characters in its middle instead.
 */
function splitTailEnd(tail: string): { name: string; end: string } {
  const units = graphemes(tail);
  let component = units.length;
  while (component > 0 && !isSeparator(units[component - 1])) component -= 1;
  const dot = units.lastIndexOf(".");
  const extension =
    dot > component && units.length - dot <= 12 ? dot : undefined;
  const start =
    extension ??
    units.length - Math.min(6, Math.floor((units.length - component) / 3));
  return {
    name: units.slice(0, start).join(""),
    end: units.slice(start).join(""),
  };
}

export const ELLIPSIS = "\u2026";

/** Where each component of `tail` after its first starts, at its separator, in order. */
function componentStarts(tail: string): number[] {
  let end = tail.length;
  while (end > 0 && isSeparator(tail[end - 1])) end -= 1;
  const starts: number[] = [];
  for (let at = 1; at < end; at += 1)
    if (isSeparator(tail[at])) starts.push(at);
  return starts;
}

/** The largest count up to `most` that `fits`; 0 when none does. */
function largest(most: number, fits: (count: number) => boolean): number {
  let low = 0;
  let high = most;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (fits(middle)) low = middle;
    else high = middle - 1;
  }
  return low;
}

/** The first `count` characters of `units`, without white space before a "…". */
function kept(units: readonly string[], count: number): string {
  return units.slice(0, count).join("").trimEnd();
}

/**
 * How a line too narrow for its text draws it: the characters the head keeps
 * (null when the head is hidden), where the shown part of the tail starts,
 * and the characters its name keeps (null when the name is whole).
 */
type Fitted = { head: number | null; start: number; name: number | null };

/** Fits `text` into `room` by whole characters; undefined when it fits as it is. */
export function fitLine(
  width: (value: string) => number,
  room: number,
  text: string,
  head: string,
  tail: string,
): Fitted | undefined {
  if (width(text) <= room) return undefined;
  if (width(ELLIPSIS + tail) <= room) {
    const units = graphemes(head);
    const count = largest(
      units.length,
      (count) => width(kept(units, count) + ELLIPSIS + tail) <= room,
    );
    return { head: count, start: 0, name: null };
  }
  // Keep as many parent folders of the tail as fit, down to the last component.
  const starts = componentStarts(tail);
  for (const at of starts)
    if (width(ELLIPSIS + tail.slice(at)) <= room)
      return { head: null, start: at, name: null };
  const start = starts.at(-1) ?? 0;
  const parts = splitTailEnd(tail.slice(start));
  const units = graphemes(parts.name);
  const count = largest(
    units.length,
    (count) =>
      width(ELLIPSIS + kept(units, count) + ELLIPSIS + parts.end) <= room,
  );
  return { head: null, start, name: count };
}

/**
 * Fits a text wider than its line by whole characters, measured with `ruler`;
 * undefined while the text fits or cannot be measured, when the styles cut
 * it instead.
 *
 * The head keeps what fits before "…" and the tail. When even "…" and the
 * tail are too wide, the head is hidden with as many parent folders of the
 * tail as needed, and the line starts with "…" before a separator, so a
 * hidden head is never read as a path from the top. A last component still
 * too wide keeps what fits of its name before "…" and its end. Each "…" meets
 * the text after it with no gap (F02 README #170 gallery review).
 */
export function useFittedLine(
  line: RefObject<HTMLSpanElement | null>,
  ruler: RefObject<HTMLSpanElement | null>,
  text: string,
  head: string,
  tail: string,
) {
  const [fitted, setFitted] = useState<Fitted>();
  useLayoutEffect(() => {
    // The frame fills its line, whatever the parts inside it measure.
    const frame = line.current?.parentElement;
    const measure = ruler.current;
    if (!frame || !measure || tail === "") return;
    const width = (value: string) => {
      measure.textContent = value;
      return measure.getBoundingClientRect().width;
    };
    const fit = () => {
      const room = frame.getBoundingClientRect().width;
      if (room <= 0) return;
      const next = fitLine(width, room, text, head, tail);
      setFitted((previous) =>
        previous?.head === next?.head &&
        previous?.start === next?.start &&
        previous?.name === next?.name
          ? previous
          : next,
      );
    };
    fit();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(fit);
    observer.observe(frame);
    return () => observer.disconnect();
  }, [line, ruler, text, head, tail]);
  return fitted;
}

/** The texts a line draws for `fitted`, with their offsets in the whole text. */
export function drawnParts(
  head: string,
  tail: string,
  fitted: Fitted | undefined,
) {
  const start = fitted?.start ?? 0;
  const parts = splitTailEnd(tail.slice(start));
  const fittedHead = fitted?.head;
  const fittedName = fitted?.name;
  return {
    head:
      typeof fittedHead === "number" ? kept(graphemes(head), fittedHead) : head,
    headCut: typeof fittedHead === "number",
    headHidden: fittedHead === null,
    name:
      typeof fittedName === "number"
        ? kept(graphemes(parts.name), fittedName)
        : parts.name,
    nameCut: typeof fittedName === "number",
    nameOffset: head.length + start,
    end: parts.end,
    endOffset: head.length + start + parts.name.length,
  };
}
