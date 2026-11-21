import { useEffect, useLayoutEffect, useRef, useState } from "react";
import "./chart-card.css";

/** What a shape's info card says (F06 README 8): a title, then one fact a line. */
export type ShapeCard = { title: string; lines: readonly string[] };

/** A pointer opens the card after resting this long; focus opens it at once (README 8). */
const HOVER_DELAY_MS = 300;
/** How long a pointer may take to cross from the shape onto the card. */
const LEAVE_GRACE_MS = 150;
/** The gap between the shape and the card, and the card and the window's edge. */
const GAP = 8;

type Open = {
  shapeRef: string;
  anchor: HTMLElement;
  source: "pointer" | "focus";
};

/**
 * Opens and closes the info card of the chart's shapes (README 8, 22): after a
 * pointer rests 0.3 s on a shape, at once when a shape takes the focus; a
 * pointer may move onto the card and it stays; Esc closes it.
 */
export function useShapeCard() {
  const [open, setOpen] = useState<Open | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const onCard = useRef(false);
  const clear = () => {
    if (timer.current !== undefined) window.clearTimeout(timer.current);
    timer.current = undefined;
  };
  useEffect(() => clear, []);
  const closePointer = () =>
    setOpen((current) => (current?.source === "pointer" ? null : current));
  return {
    open,
    hover(shapeRef: string, anchor: HTMLElement) {
      clear();
      timer.current = window.setTimeout(
        () => setOpen({ shapeRef, anchor, source: "pointer" }),
        HOVER_DELAY_MS,
      );
    },
    leave() {
      clear();
      timer.current = window.setTimeout(() => {
        if (!onCard.current) closePointer();
      }, LEAVE_GRACE_MS);
    },
    focus(shapeRef: string, anchor: HTMLElement) {
      clear();
      setOpen({ shapeRef, anchor, source: "focus" });
    },
    blur() {
      clear();
      setOpen((current) => (current?.source === "focus" ? null : current));
    },
    /** Closes the card; false when none was open, so Esc goes on to its next meaning. */
    close() {
      clear();
      if (!open) return false;
      setOpen(null);
      return true;
    },
    enterCard() {
      onCard.current = true;
      clear();
    },
    leaveCard() {
      onCard.current = false;
      closePointer();
    },
  };
}

/**
 * The card beside its shape, in the visible window (README 22): above right of
 * the shape first, then above left, below right and below left, so it covers
 * neither the shape nor its focus ring; when none fits, a fixed place in the
 * window. It holds nothing to press and is not read out: the shape's name says
 * the same (README 8).
 */
export function ShapeInfoCard({
  card,
  anchor,
  area,
  onPointerEnter,
  onPointerLeave,
}: {
  card: ShapeCard;
  anchor: HTMLElement;
  /** The chart's drawing: the card stays inside it, off the controls around (README 49). */
  area: Element | null;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{
    left: number;
    top: number;
    fixed: boolean;
  } | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    setPlace(
      cardPlace(anchor.getBoundingClientRect(), element, visibleArea(area)),
    );
  }, [anchor, area, card]);
  return (
    <div
      ref={ref}
      className="du-chart-card"
      aria-hidden="true"
      data-fixed={place?.fixed || undefined}
      style={{
        left: place?.left ?? 0,
        top: place?.top ?? 0,
        visibility: place ? "visible" : "hidden",
      }}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
    >
      <p className="du-chart-card__title">{card.title}</p>
      {card.lines.map((line, index) => (
        <p key={index} className="du-chart-card__line">
          {line}
        </p>
      ))}
    </div>
  );
}

/** The chart's drawing as far as the window shows it; the window without one. */
function visibleArea(area: Element | null) {
  const view = {
    left: 0,
    top: 0,
    right: window.innerWidth,
    bottom: window.innerHeight,
  };
  if (!area) return view;
  const box = area.getBoundingClientRect();
  return {
    left: Math.max(view.left, box.left),
    top: Math.max(view.top, box.top),
    right: Math.min(view.right, box.right),
    bottom: Math.min(view.bottom, box.bottom),
  };
}

/**
 * The first of the four places that fits inside the chart's visible drawing,
 * else the fixed place at its lower right (README 22, 49). Neighbouring shapes
 * may be covered; the format buttons and status lines around the chart are not.
 */
export function cardPlace(
  shape: Pick<DOMRect, "left" | "top" | "right" | "bottom">,
  card: Pick<HTMLElement, "offsetWidth" | "offsetHeight">,
  area: { left: number; top: number; right: number; bottom: number },
) {
  const width = card.offsetWidth;
  const height = card.offsetHeight;
  const places = [
    { left: shape.right + GAP, top: shape.top - height - GAP },
    { left: shape.left - width - GAP, top: shape.top - height - GAP },
    { left: shape.right + GAP, top: shape.bottom + GAP },
    { left: shape.left - width - GAP, top: shape.bottom + GAP },
  ];
  const fits = ({ left, top }: { left: number; top: number }) =>
    left >= area.left + GAP &&
    top >= area.top + GAP &&
    left + width <= area.right - GAP &&
    top + height <= area.bottom - GAP;
  const found = places.find(fits);
  return found
    ? { ...found, fixed: false }
    : {
        left: Math.max(area.left + GAP, area.right - width - GAP),
        top: Math.max(area.top + GAP, area.bottom - height - GAP),
        fixed: true,
      };
}
