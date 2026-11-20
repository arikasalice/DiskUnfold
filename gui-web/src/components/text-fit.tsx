import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode, RefObject } from "react";
import { graphemes } from "./graphemes";
import { isImeProcessingKey } from "./ime-composition";
import { NAME_SPACE_MARK, pathNameEdges } from "./name-edges";
import { NameText, hasPathSeparator } from "./name-text";
import "../theme/text-fit.css";
import "../theme/name-text.css";

/** Longest tail, in characters, kept when a text has no path structure to follow. */
const MIDDLE_TAIL_LIMIT = 16;
/** Longest last path component, in characters, that the tail keeps whole. */
const TAIL_NAME_LIMIT = 32;
/** The tail also keeps the parent folder while it stays within this many characters. */
const TAIL_FOLDER_BUDGET = 24;

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
 *
 * With `focusable` false, as in a table cell, a cut frame takes no tab stop
 * and no role; the full text shows after the pointer rests on it for a moment,
 * and the content still carries the whole text for assistive technology.
 */
function CutTextFrame({
  text,
  full = text,
  cut,
  className,
  wholeCopy = false,
  focusable = true,
  children,
}: {
  text: string;
  /** The full text as shown in the aid; `text` by default. */
  full?: ReactNode;
  cut: boolean;
  className: string;
  wholeCopy?: boolean;
  focusable?: boolean;
  children: ReactNode;
}) {
  const [dismissed, setDismissed] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const frame = useRef<HTMLSpanElement>(null);
  const [anchor, setAnchor] = useState<CSSProperties>();
  const shown = cut && (hovered || focused) && !dismissed;

  // A table cell clips what overflows it, so there the full text is placed
  // against the window, under the cell, and follows it while the page scrolls.
  const place = () => {
    const box = frame.current?.getBoundingClientRect();
    if (box)
      setAnchor({
        "--du-fit-top": box.bottom + "px",
        "--du-fit-left": box.left + "px",
      } as CSSProperties);
  };
  const placing = cut && !focusable && hovered;
  useEffect(() => {
    if (!placing) return;
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  }, [placing]);

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
      ref={frame}
      className={`du-fit ${className}${focusable ? "" : " du-fit--cell"}`}
      tabIndex={cut && focusable ? 0 : undefined}
      role={cut && focusable ? "note" : undefined}
      data-dismissed={dismissed || undefined}
      onMouseEnter={() => {
        setHovered(true);
        if (!focusable) place();
      }}
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
        <span
          className="du-fit__full"
          dir="auto"
          aria-hidden
          style={focusable ? undefined : anchor}
        >
          {full}
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

/** UTF-16 ranges of the edge white space of a path's last component. */
function edgeRanges(text: string): [number, number][] {
  const { head, name } = pathNameEdges(text);
  const start = head.length;
  const trailing = start + name.leading.length + name.middle.length;
  return [
    [start, start + name.leading.length],
    [trailing, trailing + name.trailing.length],
  ];
}

/**
 * `part`, which starts at `offset` in its text, with the characters inside
 * `ranges` kept for copying and drawn as "\u2423" marks.
 */
function marked(
  part: string,
  offset: number,
  ranges: readonly [number, number][],
): ReactNode {
  const nodes: ReactNode[] = [];
  let at = 0;
  for (const [from, to] of ranges) {
    const start = Math.max(from - offset, at);
    const end = Math.min(to - offset, part.length);
    if (start >= end) continue;
    if (start > at) nodes.push(part.slice(at, start));
    const spaces = part.slice(start, end);
    nodes.push(
      <span key={start}>
        <span className="du-name__space">{spaces}</span>
        <span className="du-name__mark">
          {NAME_SPACE_MARK.repeat(Array.from(spaces).length)}
        </span>
      </span>,
    );
    at = end;
  }
  if (nodes.length === 0) return part;
  if (at < part.length) nodes.push(part.slice(at));
  return nodes;
}

/**
 * A path or file name on one line with the ellipsis in the middle.
 *
 * The visible parts are hidden from assistive technology and a visually
 * hidden copy carries the full original text as the accessible name. Once the
 * text is cut, the element takes focus and shows the full text, and a copy of
 * the selection gives the full path once.
 *
 * The last component keeps its extension: when it is wider than the line by
 * itself, its middle is cut too. The edge white space of the last component
 * is drawn as "\u2423" on screen and in the full text, and screen readers hear
 * the explanation after the path, as `NameText` reads it.
 *
 * With `cell`, for a table cell, a cut text takes no tab stop: the row's own
 * details give keyboard users the full value (F02 README #170).
 *
 * The container takes its direction from the whole text (`dir="auto"`) and
 * both parts are bidi isolates that inherit it, so the two flex items stand in
 * the order of the unsplit text and a right-to-left name does not swap sides.
 */
export function MiddleEllipsisText({
  text,
  cell = false,
}: {
  text: string;
  /** Inside a table cell: no tab stop when cut (F02 README #170). */
  cell?: boolean;
}) {
  const head = useRef<HTMLSpanElement>(null);
  const tail = useRef<HTMLSpanElement>(null);
  const tailName = useRef<HTMLSpanElement>(null);
  const refs = useRef([head, tail, tailName]).current;
  const cut = useOverflow(refs, text);
  const parts = splitForMiddleEllipsis(text);
  const tailParts = splitTailEnd(parts.tail);
  const ranges = edgeRanges(text);
  const path = hasPathSeparator(text);
  return (
    <CutTextFrame
      text={text}
      full={<NameText name={text} path={path} />}
      cut={cut}
      className="du-fit--line"
      wholeCopy
      focusable={!cell}
    >
      <span className="du-middle" dir="auto">
        <span ref={head} className="du-middle__head" aria-hidden>
          {marked(parts.head, 0, ranges)}
        </span>
        <span ref={tail} className="du-middle__tail" aria-hidden>
          <span ref={tailName} className="du-middle__name">
            {marked(tailParts.name, parts.head.length, ranges)}
          </span>
          <span className="du-middle__end">
            {marked(
              tailParts.end,
              parts.head.length + tailParts.name.length,
              ranges,
            )}
          </span>
        </span>
      </span>
      <span className="du-visually-hidden" dir="auto">
        <NameText name={text} path={path} />
      </span>
    </CutTextFrame>
  );
}
