import { useEffect, useState } from "react";
import type { RefObject } from "react";

function selectionWithin(
  areas: readonly RefObject<HTMLElement | null>[],
): boolean {
  const selection = document.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0)
    return false;
  const range = selection.getRangeAt(0);
  return areas.some((area) => {
    const node = area.current;
    return node !== null && range.intersectsNode(node);
  });
}

/**
 * Whether the user is selecting text inside any of the given areas.
 *
 * A screen that changes its values in place holds them while this is true,
 * so the text the user is about to copy stays put and never mixes values
 * from two results. The hold ends as soon as the selection collapses or
 * leaves the areas. The areas are read when the selection changes; the
 * caller passes stable refs.
 */
export function useSelectionHold(
  areas: readonly RefObject<HTMLElement | null>[],
): boolean {
  const [holding, setHolding] = useState(false);
  useEffect(() => {
    const check = () => setHolding(selectionWithin(areas));
    document.addEventListener("selectionchange", check);
    return () => document.removeEventListener("selectionchange", check);
    // The refs are stable; their targets are read at each change.
  }, []);
  return holding;
}

/**
 * Returns `value`, except that while `hold` is true it keeps returning the
 * value it returned when the hold began.
 */
export function useHeldValue<T>(value: T, hold: boolean): T {
  const [held, setHeld] = useState(value);
  if (!hold && held !== value) setHeld(value);
  return hold ? held : value;
}
