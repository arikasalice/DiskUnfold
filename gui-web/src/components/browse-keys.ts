import type { KeyboardEvent } from "react";

const editingOrPopup =
  'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"], [role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], [role="combobox"]';
const activePopup =
  'dialog[open], [role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], [role="combobox"][aria-expanded="true"]';

function isVisiblePopup(popup: Element): boolean {
  if (popup.closest('[hidden], [inert], [aria-hidden="true"]')) return false;
  if (getComputedStyle(popup).visibility === "hidden") return false;
  for (
    let ancestor: Element | null = popup;
    ancestor;
    ancestor = ancestor.parentElement
  ) {
    if (getComputedStyle(ancestor).display === "none") return false;
  }
  return true;
}

/** Native document scrolling is observed only outside editing and popups. */
export function canObserveBrowseKey(event: globalThis.KeyboardEvent): boolean {
  const target = event.target;
  if (
    event.defaultPrevented ||
    event.isComposing ||
    event.keyCode === 229 ||
    !(target instanceof Element) ||
    target.closest(editingOrPopup)
  ) {
    return false;
  }

  return !Array.from(target.ownerDocument.querySelectorAll(activePopup)).some(
    isVisiblePopup,
  );
}

/** Navigation commands also require focus inside the browsing surface. */
export function canHandleBrowseKey(event: KeyboardEvent<HTMLElement>): boolean {
  return (
    !event.defaultPrevented &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.shiftKey &&
    event.target instanceof Element &&
    event.currentTarget.contains(event.target) &&
    canObserveBrowseKey(event.nativeEvent)
  );
}
