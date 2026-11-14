import { useEffect, useMemo, useRef, useState } from "react";
import type { ContextType, RefObject } from "react";
import { Tooltip, TooltipTriggerStateContext } from "react-aria-components";
import { useLocaleText } from "../i18n/presentation";
import type { LocaleText } from "../i18n/presentation";
import { isImeProcessingKey } from "./ime-composition";
import { NAME_SPACE_MARK, nameEdges, pathNameEdges } from "./name-edges";
import type { NameEdges } from "./name-edges";
import "../theme/text-fit.css";
import "../theme/name-text.css";

/**
 * Focus on one of these elements shows the explanation of a name inside it.
 * Each holds one item; a list, table region or panel that takes focus does
 * not open the explanations of every name it contains.
 */
const ITEM_FOCUS =
  'button, a[href], [role="row"], [role="rowheader"], [role="gridcell"], [role="note"], .du-text[tabindex]';

function edgeHints(edges: NameEdges, text: LocaleText): string[] {
  const hints: string[] = [];
  if (edges.leadingCount > 0)
    hints.push(
      text(
        "ui.common.name_leading_spaces",
        edges.leadingCount === 1
          ? "The name starts with {count} space"
          : "The name starts with {count} spaces",
        { count: BigInt(edges.leadingCount) },
      ),
    );
  if (edges.trailingCount > 0)
    hints.push(
      text(
        "ui.common.name_trailing_spaces",
        edges.trailingCount === 1
          ? "The name ends with {count} space"
          : "The name ends with {count} spaces",
        { count: BigInt(edges.trailingCount) },
      ),
    );
  return hints;
}

/**
 * Opens the explanation while the pointer is on the name or focus is on the
 * item that holds it. Escape hides it until both have left.
 */
function useNameHint(name: RefObject<HTMLElement | null>, active: boolean) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const hovering = useRef(false);
  const open = active && (hovered || focused) && !dismissed;
  // The tooltip reads its open state from the trigger context, which this
  // name provides itself because no focusable trigger owns it.
  const state = useMemo<
    NonNullable<ContextType<typeof TooltipTriggerStateContext>>
  >(
    () => ({
      isOpen: open,
      shouldSkipAnimation: false,
      open: () => undefined,
      close: () => setDismissed(true),
    }),
    [open],
  );

  useEffect(() => {
    if (!active) return;
    const holds = (target: EventTarget | null) =>
      target instanceof Element &&
      target.matches(ITEM_FOCUS) &&
      name.current !== null &&
      target.contains(name.current);
    const focusIn = (event: FocusEvent) => setFocused(holds(event.target));
    const focusOut = () => {
      setFocused(false);
      if (!hovering.current) setDismissed(false);
    };
    document.addEventListener("focusin", focusIn);
    document.addEventListener("focusout", focusOut);
    return () => {
      document.removeEventListener("focusin", focusIn);
      document.removeEventListener("focusout", focusOut);
    };
  }, [active, name]);

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

  return {
    state,
    enter: () => {
      hovering.current = true;
      setHovered(true);
    },
    leave: () => {
      hovering.current = false;
      setHovered(false);
      if (!focused) setDismissed(false);
    },
  };
}

/** The real edge white space, kept for copying, and the marks drawn in its place. */
function Edge({ characters, count }: { characters: string; count: number }) {
  if (count === 0) return null;
  return (
    <>
      <span className="du-name__space">{characters}</span>
      <span className="du-name__mark" aria-hidden="true">
        {NAME_SPACE_MARK.repeat(count)}
      </span>
    </>
  );
}

/**
 * A file or folder name, or a path whose last component is a name, with its
 * edge white space made visible.
 *
 * Each white space character at the start or end of the name is drawn as a
 * muted "\u2423" at the size of the name. The marks are hidden from
 * assistive technology and from text selection, and the name keeps its real
 * characters, so the text content and a copied selection are the original
 * value. Pass the real name: search, sorting, comparison, copying and IPC
 * keep using that value, never the drawn marks.
 *
 * When the name has edge white space, an explanation such as "The name ends
 * with 1 space" follows the name for screen readers and appears beside it on
 * hover or when focus is on the item that holds the name. A name without edge
 * white space renders as the plain text it was before, with no element of its
 * own and no listener.
 *
 * With `path`, only the last component is treated as a name. The name takes
 * its own direction from its first strong character, so the marks of a
 * right-to-left name stand at its logical start and end.
 */
export function NameText({
  name,
  path = false,
}: {
  name: string;
  /** Treats `name` as a path and marks only its last component. */
  path?: boolean;
}) {
  const text = useLocaleText();
  const ref = useRef<HTMLSpanElement>(null);
  const parts = path
    ? pathNameEdges(name)
    : { head: "", name: nameEdges(name), tail: "" };
  const hints = edgeHints(parts.name, text);
  const hint = useNameHint(ref, hints.length > 0);
  if (hints.length === 0) return name;
  const edges = parts.name;
  return (
    <span
      ref={ref}
      className={path ? "du-name du-name--path" : "du-name"}
      onPointerEnter={hint.enter}
      onPointerLeave={hint.leave}
    >
      {parts.head}
      <span className="du-name__text" dir="auto">
        <Edge characters={edges.leading} count={edges.leadingCount} />
        {edges.middle}
        <Edge characters={edges.trailing} count={edges.trailingCount} />
      </span>
      {parts.tail}
      <span className="du-visually-hidden">{`, ${hints.join(", ")}`}</span>
      <TooltipTriggerStateContext.Provider value={hint.state}>
        <Tooltip
          triggerRef={ref}
          placement="top"
          offset={6}
          className="du-name-hint"
        >
          {hints.map((line) => (
            <span key={line}>{line}</span>
          ))}
        </Tooltip>
      </TooltipTriggerStateContext.Provider>
    </span>
  );
}
