import type { KeyboardEvent } from "react";
import { classifyKeyStage } from "./key-dispatch";
import { createKeyTable } from "./key-table";
import type { KeyBinding, KeyModifier } from "./key-table";

/** The screen identifier of the folder and file browsing surface. */
export const BROWSE_SCREEN = "browse";

function binding(
  key: string,
  action: string,
  modifiers: readonly KeyModifier[] = [],
): KeyBinding {
  return {
    screen: BROWSE_SCREEN,
    key,
    modifiers,
    action,
    descriptionKey: `ui.browse.keys.${action}`,
  };
}

/** The browsing surface's key assignment table, by logical key. */
export const browseKeyTable = createKeyTable([
  binding("ArrowUp", "move_previous"),
  binding("ArrowDown", "move_next"),
  binding("ArrowLeft", "cell_previous"),
  binding("ArrowRight", "cell_next"),
  binding("Home", "move_first"),
  binding("End", "move_last"),
  binding("PageUp", "move_page_up"),
  binding("PageDown", "move_page_down"),
  binding("Enter", "open_item"),
  binding(" ", "toggle_selection"),
  binding("ArrowUp", "go_up", ["alt"]),
]);

/** Native document scrolling is observed only when the screen owns the key. */
export function canObserveBrowseKey(event: globalThis.KeyboardEvent): boolean {
  return (
    !event.defaultPrevented &&
    event.target instanceof Element &&
    classifyKeyStage(event) === "screen"
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
