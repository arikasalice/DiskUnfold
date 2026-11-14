import { isImeProcessingKey } from "./ime-composition";
import type { KeyEventLike, KeyTable } from "./key-table";

const editing =
  'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"], [role="combobox"]';
const popup =
  '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], [role="combobox"]';
const activePopup =
  'dialog[open], [role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], [role="combobox"][aria-expanded="true"]';

/** The first stage that owns a key press, in processing order. */
export type KeyStage = "composition" | "input" | "dialog" | "screen";

/** Where a key press ended up. `table` carries the matched action. */
export type KeyDispatch =
  | Readonly<{ stage: "composition" | "input" | "dialog" | "ignored" }>
  | Readonly<{ stage: "table"; action: string }>;

/** The subset of a native keyboard event the dispatcher reads. */
export type DispatchedKey = KeyEventLike &
  Readonly<{
    isComposing?: boolean;
    keyCode?: number;
    defaultPrevented: boolean;
    target: EventTarget | null;
  }>;

function isVisiblePopup(element: Element): boolean {
  if (element.closest('[hidden], [inert], [aria-hidden="true"]')) return false;
  if (getComputedStyle(element).visibility === "hidden") return false;
  for (
    let ancestor: Element | null = element;
    ancestor;
    ancestor = ancestor.parentElement
  ) {
    if (getComputedStyle(ancestor).display === "none") return false;
  }
  return true;
}

/**
 * Decides which stage owns a key press: input method composition first, then
 * a text entry, then a dialog or menu, and otherwise the screen. The order is
 * fixed here so no screen re-derives it. A press whose target is not an
 * element stays with the screen stage and is later ignored.
 */
export function classifyKeyStage(event: DispatchedKey): KeyStage {
  if (isImeProcessingKey(event)) return "composition";
  const target = event.target;
  if (!(target instanceof Element)) return "screen";
  if (target.closest(editing)) return "input";
  if (target.closest(popup)) return "dialog";
  const open = Array.from(target.ownerDocument.querySelectorAll(activePopup));
  return open.some(isVisiblePopup) ? "dialog" : "screen";
}

/**
 * Handles one key press in one place: composition, input, dialog, the
 * screen's key assignment table, then ignore. Only the table stage yields an
 * action. A press that another handler already cancelled, or whose target is
 * not an element, is ignored.
 */
export function dispatchKey(
  event: DispatchedKey,
  table: KeyTable,
  screen: string,
): KeyDispatch {
  const stage = classifyKeyStage(event);
  if (stage !== "screen") return { stage };
  if (event.defaultPrevented || !(event.target instanceof Element))
    return { stage: "ignored" };
  const binding = table.find(screen, event);
  return binding
    ? { stage: "table", action: binding.action }
    : { stage: "ignored" };
}
