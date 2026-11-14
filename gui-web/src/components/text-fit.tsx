import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode, RefObject } from "react";
import { isImeProcessingKey } from "./ime-composition";
import "../theme/text-fit.css";

/** Longest tail, in characters, kept when a text has no path structure to follow. */
const MIDDLE_TAIL_LIMIT = 16;
/** Longest last path component, in characters, that the tail keeps whole. */
const TAIL_NAME_LIMIT = 32;
/** The tail also keeps the parent folder while it stays within this many characters. */
const TAIL_FOLDER_BUDGET = 24;

/**
 * Splits text into user-perceived characters (extended grapheme clusters), so
 * a base letter and its combining marks, a conjoining Hangul jamo sequence, a
 * ZWJ emoji or a surrogate pair is never cut in the middle. Without
 * `Intl.Segmenter` it falls back to code points.
 */
function graphemes(text: string): string[] {
  if (typeof Intl !== "undefined" && typeof Intl.Segmenter === "function") {
    const segmenter = new Intl.Segmenter("und", { granularity: "grapheme" });
    return Array.from(segmenter.segment(text), (part) => part.segment);
  }
  return Array.from(text);
}

function isSeparator(unit: string | undefined): boolean {
  return unit === "/" || unit === "\\";
}

/**
 * Where the tail of a path starts: at the separator before the last component,
 * extended by the parent folder when both are short. Undefined when the text
 * has no separator to cut at or its last component is too long to keep whole.
 */
function pathTailStart(units: readonly string[]): number | undefined {
  let end = units.length;
  while (end > 0 && isSeparator(units[end - 1])) end -= 1;
  let name = end;
  while (name > 0 && !isSeparator(units[name - 1])) name -= 1;
  if (name <= 1 || units.length - name > TAIL_NAME_LIMIT) return undefined;
  // Keep the separator that precedes the last component with the tail.
  let start = name - 1;
  while (start > 0) {
    let previous = start;
    while (previous > 0 && isSeparator(units[previous - 1])) previous -= 1;
    while (previous > 0 && !isSeparator(units[previous - 1])) previous -= 1;
    const candidate = Math.max(previous - 1, 0);
    if (candidate <= 0 || units.length - candidate > TAIL_FOLDER_BUDGET) break;
    start = candidate;
  }
  return start;
}

/** True while an element's content is wider or taller than its box. */
function isCut(element: HTMLElement | null): boolean {
  return (
    element !== null &&
    (element.scrollHeight > element.clientHeight + 1 ||
      element.scrollWidth > element.clientWidth + 1)
  );
}

/** True while any of the elements shows less than its full content. */
function useOverflow(
  refs: readonly RefObject<HTMLElement | null>[],
  text: string,
) {
  const [overflowing, setOverflowing] = useState(false);
  useLayoutEffect(() => {
    const elements = refs.flatMap((ref) => (ref.current ? [ref.current] : []));
    if (elements.length === 0) return;
    const measure = () => setOverflowing(elements.some(isCut));
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    for (const element of elements) observer.observe(element);
    return () => observer.disconnect();
  }, [refs, text]);
  return overflowing;
}

/**
 * Frames a cut-off text. Once `cut`, the frame takes keyboard focus and shows
 * the full text beside it on focus or hover. Escape hides it, whether it came
 * from focus or from the pointer, until both have left.
 *
 * The full text is only a visual aid: the frame's content already carries the
 * whole text for assistive technology, so the aid is hidden from it instead of
 * being read a second time. A cut frame has `role="note"`. A focusable element
 * with no role leaves a screen reader to guess what the tab stop is, and
 * `Focusable` or a button role would announce an action that does not exist.
 * A note is read as the text it holds. The frame is therefore a plain element,
 * not a React Aria control: it has no press behavior to wrap. The role is set
 * only while the text is cut, so uncut text adds no tab stop and no
 * announcement.
 *
 * With `wholeCopy`, copying a selection that lies inside the frame puts the
 * full text on the clipboard once, whatever part of it is visible.
 */
function CutTextFrame({
  text,
  cut,
  className,
  wholeCopy = false,
  children,
}: {
  text: string;
  cut: boolean;
  className: string;
  wholeCopy?: boolean;
  children: ReactNode;
}) {
  const [dismissed, setDismissed] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const shown = cut && (hovered || focused) && !dismissed;

  // A pointer-opened aid has no focus to receive Escape, so it listens on the
  // window while it is shown and takes the key before a dialog sees it.
  useEffect(() => {
    if (!shown) return;
    const dismiss = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || isImeProcessingKey(event)) return;
      event.stopPropagation();
      setDismissed(true);
    };
    window.addEventListener("keydown", dismiss, true);
    return () => window.removeEventListener("keydown", dismiss, true);
  }, [shown]);

  return (
    <span
      className={`du-fit ${className}`}
      tabIndex={cut ? 0 : undefined}
      role={cut ? "note" : undefined}
      data-dismissed={dismissed || undefined}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => {
        setHovered(false);
        if (!focused) setDismissed(false);
      }}
      onFocus={() => setFocused(true)}
      onBlur={() => {
        setFocused(false);
        if (!hovered) setDismissed(false);
      }}
      onKeyDown={(event) => {
        if (
          cut &&
          !dismissed &&
          event.key === "Escape" &&
          !isImeProcessingKey(event.nativeEvent)
        ) {
          event.stopPropagation();
          setDismissed(true);
        }
      }}
      onCopy={(event) => {
        if (!wholeCopy) return;
        const selection = window.getSelection();
        const frame = event.currentTarget;
        if (
          selection &&
          !selection.isCollapsed &&
          frame.contains(selection.anchorNode) &&
          frame.contains(selection.focusNode)
        ) {
          event.clipboardData.setData("text/plain", text);
          event.preventDefault();
        }
      }}
    >
      {children}
      {cut && (
        <span className="du-fit__full" dir="auto" aria-hidden>
          {text}
        </span>
      )}
    </span>
  );
}

/**
 * Prose that wraps first and ends with an ellipsis after `lines` lines.
 *
 * The element has no fixed width. The whole text stays in the content, so the
 * accessible name is the full text. Once the text is cut, the element takes
 * focus and shows the full text.
 */
export function ClampedText({
  text,
  lines = 2,
}: {
  text: string;
  /** Lines shown before the ellipsis. The design limit is two. */
  lines?: number;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const refs = useRef([ref]).current;
  const cut = useOverflow(refs, text);
  const style = { "--du-clamp-lines": lines } as CSSProperties;
  return (
    <CutTextFrame text={text} cut={cut} className="du-fit--block">
      <span ref={ref} className="du-clamp" style={style}>
        {text}
      </span>
    </CutTextFrame>
  );
}

/**
 * Splits a path or file name for a middle ellipsis.
 *
 * Both parts always join back into `text` and the cut never falls inside a
 * grapheme cluster. For a path the tail is the last component with its leading
 * separator (`/` or `\`), plus the parent folder when both are short, so the
 * file or folder name stays whole and the head, which starts at the drive or
 * root, is the part that shrinks. A name with no folder, or a last component
 * longer than the tail limit, keeps its final characters, which usually carry
 * the extension. Text of 16 characters or fewer is not split.
 */
export function splitForMiddleEllipsis(text: string): {
  head: string;
  tail: string;
} {
  const units = graphemes(text);
  if (units.length <= MIDDLE_TAIL_LIMIT) return { head: text, tail: "" };
  const start =
    pathTailStart(units) ??
    units.length - Math.min(MIDDLE_TAIL_LIMIT, Math.floor(units.length / 3));
  return {
    head: units.slice(0, start).join(""),
    tail: units.slice(start).join(""),
  };
}

/**
 * A path or file name on one line with the ellipsis in the middle.
 *
 * The visible parts are hidden from assistive technology and a visually
 * hidden copy carries the full original text as the accessible name. Once the
 * text is cut, the element takes focus and shows the full text, and a copy of
 * the selection gives the full path once.
 *
 * The container takes its direction from the whole text (`dir="auto"`) and
 * both parts are bidi isolates that inherit it, so the two flex items stand in
 * the order of the unsplit text and a right-to-left name does not swap sides.
 */
export function MiddleEllipsisText({ text }: { text: string }) {
  const head = useRef<HTMLSpanElement>(null);
  const tail = useRef<HTMLSpanElement>(null);
  const refs = useRef([head, tail]).current;
  const cut = useOverflow(refs, text);
  const parts = splitForMiddleEllipsis(text);
  return (
    <CutTextFrame text={text} cut={cut} className="du-fit--line" wholeCopy>
      <span className="du-middle" dir="auto">
        <span ref={head} className="du-middle__head" aria-hidden>
          {parts.head}
        </span>
        <span ref={tail} className="du-middle__tail" aria-hidden>
          {parts.tail}
        </span>
      </span>
      <span className="du-visually-hidden" dir="auto">
        {text}
      </span>
    </CutTextFrame>
  );
}
