import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ContextType, ReactNode, RefObject } from "react";
import { Tooltip, TooltipTriggerStateContext } from "react-aria-components";
import { isImeProcessingKey } from "./ime-composition";

/**
 * Focus on one of these elements shows the explanation of a text inside it.
 * Each holds one item; a list, table region or panel that takes focus does
 * not open the explanations of everything it contains.
 */
const ITEM_FOCUS =
  'button, a[href], [role="row"], [role="rowheader"], [role="gridcell"], [role="note"], .du-text[tabindex]';

/** How long an explanation stays after the pointer left, so the pointer can reach it. */
const HOVER_GRACE_MS = 300;

type HintState = NonNullable<ContextType<typeof TooltipTriggerStateContext>>;

/**
 * Opens an explanation while the pointer is on `target` or focus is on the
 * item that holds it, which may be `target` itself. Escape hides it until
 * both have left.
 *
 * The pointer can also move onto the explanation: it stays while the pointer
 * is on it and for a short time after the pointer left `target` or the
 * explanation.
 *
 * Put `enter` and `leave` on the pointer events of `target` and give the
 * result to `HintPopup`.
 */
export function useHint(
  target: RefObject<HTMLElement | null>,
  active: boolean,
) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const hovering = useRef(false);
  const holdsFocus = useRef(false);
  const leaving = useRef<number | undefined>(undefined);
  const open = active && (hovered || focused) && !dismissed;

  const keep = useCallback(() => {
    if (leaving.current === undefined) return;
    window.clearTimeout(leaving.current);
    leaving.current = undefined;
  }, []);
  const enter = useCallback(() => {
    keep();
    hovering.current = true;
    setHovered(true);
  }, [keep]);
  const leave = useCallback(() => {
    keep();
    leaving.current = window.setTimeout(() => {
      leaving.current = undefined;
      hovering.current = false;
      setHovered(false);
      if (!holdsFocus.current) setDismissed(false);
    }, HOVER_GRACE_MS);
  }, [keep]);
  useEffect(() => keep, [keep]);

  // The tooltip reads its open state from the trigger context, which this
  // hook provides itself because no focusable trigger owns it. The tooltip
  // reports the pointer on itself through open() and close(), and a scroll
  // of the target through close(true).
  const state = useMemo<HintState>(
    () => ({
      isOpen: open,
      shouldSkipAnimation: false,
      open: enter,
      close: (immediate) => {
        if (immediate) setDismissed(true);
        else leave();
      },
    }),
    [open, enter, leave],
  );

  useEffect(() => {
    if (!active) return;
    const holds = (focus: EventTarget | null) =>
      focus instanceof Element &&
      focus.matches(ITEM_FOCUS) &&
      target.current !== null &&
      focus.contains(target.current);
    const focusIn = (event: FocusEvent) => {
      holdsFocus.current = holds(event.target);
      setFocused(holdsFocus.current);
    };
    const focusOut = () => {
      holdsFocus.current = false;
      setFocused(false);
      if (!hovering.current) setDismissed(false);
    };
    document.addEventListener("focusin", focusIn);
    document.addEventListener("focusout", focusOut);
    return () => {
      document.removeEventListener("focusin", focusIn);
      document.removeEventListener("focusout", focusOut);
    };
  }, [active, target]);

  // Neither the pointer nor the item's focus can receive Escape here, so the
  // window takes it while the explanation is shown, before a dialog sees it.
  useEffect(() => {
    if (!open) return;
    const dismiss = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || isImeProcessingKey(event)) return;
      event.stopPropagation();
      setDismissed(true);
    };
    window.addEventListener("keydown", dismiss, true);
    return () => window.removeEventListener("keydown", dismiss, true);
  }, [open]);

  return { state, enter, leave };
}

/** The explanation of `useHint`, drawn above the element `triggerRef` names. */
export function HintPopup({
  hint,
  triggerRef,
  children,
}: {
  hint: ReturnType<typeof useHint>;
  triggerRef: RefObject<HTMLElement | null>;
  children: ReactNode;
}) {
  return (
    <TooltipTriggerStateContext.Provider value={hint.state}>
      <Tooltip
        triggerRef={triggerRef}
        placement="top"
        offset={6}
        className="du-name-hint"
      >
        {children}
      </Tooltip>
    </TooltipTriggerStateContext.Provider>
  );
}
