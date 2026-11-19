import { useLayoutEffect } from "react";
import type { RefObject } from "react";
import { MIN_CHART_HEIGHT } from "../features/visualization/chart-area";

const ROOT_FLAG = "duPageScroll";

/** The window height in the CSS pixels of the zoomed root, the same value as --du-viewport-height. */
export function viewportCssHeight(): number {
  const scale = Number.parseFloat(
    getComputedStyle(document.documentElement).getPropertyValue(
      "--du-app-scale",
    ),
  );
  return window.innerHeight / (Number.isFinite(scale) && scale > 0 ? scale : 1);
}

/** Whether the whole page scrolls, banner and app head included, instead of the content row only. */
export function pageScrolls(): boolean {
  return document.documentElement.dataset[ROOT_FLAG] === "true";
}

/**
 * The height a content area gets for its layout: its own box while the head
 * stays fixed, the window when the page scrolls, since its box then grows with
 * what it holds.
 */
export function shownHeight(boxHeight: number): number {
  return pageScrolls() ? viewportCssHeight() : boxHeight;
}

function documentBottom(element: HTMLElement): number {
  let top = element.offsetHeight;
  for (let at: Element | null = element; at instanceof HTMLElement;) {
    top += at.offsetTop;
    at = at.offsetParent;
  }
  return top;
}

/**
 * Keeps the startup banner and the app head fixed above a scrolling content
 * row only while that row has room for the smallest chart (240 CSS px);
 * otherwise the whole page scrolls with one scrollbar and the head goes up with
 * it (F06 design README sections 20 item 4, 21 and 32). The room is the window
 * height in the zoomed root's CSS pixels less what the banner and the head take,
 * so the app scale counts, not the real window height. The decision becomes
 * `data-du-page-scroll` on the root, which the frame layouts read.
 */
export function usePageScroll(app: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const element = app.current;
    const head = element?.firstElementChild;
    if (!element || !(head instanceof HTMLElement)) return;
    const root = document.documentElement;
    const decide = () => {
      const room = viewportCssHeight() - documentBottom(head);
      if (room < MIN_CHART_HEIGHT) root.dataset[ROOT_FLAG] = "true";
      else delete root.dataset[ROOT_FLAG];
    };
    decide();
    const observer =
      typeof ResizeObserver === "undefined"
        ? undefined
        : new ResizeObserver(decide);
    observer?.observe(head);
    if (element.previousElementSibling)
      observer?.observe(element.previousElementSibling);
    window.addEventListener("resize", decide);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", decide);
      delete root.dataset[ROOT_FLAG];
    };
  }, [app]);
}
