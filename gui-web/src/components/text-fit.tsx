import { useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode, RefObject } from "react";
import { isImeProcessingKey } from "./ime-composition";
import "../theme/text-fit.css";

/** Longest tail, in characters, that a middle ellipsis keeps visible. */
const MIDDLE_TAIL_LIMIT = 16;

/** True while the element's content is wider or taller than its box. */
function useOverflow(ref: RefObject<HTMLElement | null>, text: string) {
  const [overflowing, setOverflowing] = useState(false);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () =>
      setOverflowing(
        element.scrollHeight > element.clientHeight + 1 ||
          element.scrollWidth > element.clientWidth + 1,
      );
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref, text]);
  return overflowing;
}

/**
 * Frames a cut-off text. Once `cut`, the frame takes keyboard focus and shows
 * the full text beside it on focus or hover; Escape hides it until the focus
 * leaves. The full text is only a visual aid: the frame's content already
 * carries the whole text for assistive technology, so the aid is hidden from it
 * instead of being read a second time.
 */
function CutTextFrame({
  text,
  cut,
  className,
  children,
}: {
  text: string;
  cut: boolean;
  className: string;
  children: ReactNode;
}) {
  const [dismissed, setDismissed] = useState(false);
  return (
    <span
      className={`du-fit ${className}`}
      tabIndex={cut ? 0 : undefined}
      data-dismissed={dismissed || undefined}
      onBlur={() => setDismissed(false)}
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
    >
      {children}
      {cut && (
        <span className="du-fit__full" aria-hidden>
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
  const cut = useOverflow(ref, text);
  const style = { "--du-clamp-lines": lines } as CSSProperties;
  return (
    <CutTextFrame text={text} cut={cut} className="du-fit--block">
      <span ref={ref} className="du-clamp" style={style}>
        {text}
      </span>
    </CutTextFrame>
  );
}

/** Splits a path or file name so the tail (usually the extension) survives. */
export function splitForMiddleEllipsis(text: string): {
  head: string;
  tail: string;
} {
  const chars = Array.from(text);
  if (chars.length <= MIDDLE_TAIL_LIMIT) return { head: text, tail: "" };
  const keep = Math.min(MIDDLE_TAIL_LIMIT, Math.floor(chars.length / 3));
  return {
    head: chars.slice(0, chars.length - keep).join(""),
    tail: chars.slice(chars.length - keep).join(""),
  };
}

/**
 * A path or file name on one line with the ellipsis in the middle.
 *
 * The visible parts are hidden from assistive technology and a visually
 * hidden copy carries the full original text as the accessible name. Once the
 * text is cut, the element takes focus and shows the full text.
 */
export function MiddleEllipsisText({ text }: { text: string }) {
  const head = useRef<HTMLSpanElement>(null);
  const cut = useOverflow(head, text);
  const parts = splitForMiddleEllipsis(text);
  return (
    <CutTextFrame text={text} cut={cut} className="du-fit--line">
      <span className="du-middle">
        <span ref={head} className="du-middle__head" aria-hidden>
          {parts.head}
        </span>
        <span className="du-middle__tail" aria-hidden>
          {parts.tail}
        </span>
      </span>
      <span className="du-visually-hidden">{text}</span>
    </CutTextFrame>
  );
}
