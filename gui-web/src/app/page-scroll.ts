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

function documentTop(element: HTMLElement): number {
  let top = 0;
  for (let at: Element | null = element; at instanceof HTMLElement;) {
    top += at.offsetTop;
    at = at.offsetParent;
  }
  return top;
}

/**
 * Where the scrolling content row starts on the page: the space view's scroll
 * area when it is shown, since its title and path sit above it, otherwise right
 * below the app head. Neither moves when the page starts or stops scrolling, so
 * the decision does not flip back and forth.
 */
function contentTop(app: HTMLElement, head: HTMLElement): number {
  const area = app.querySelector<HTMLElement>(".du-visualization-viewport");
  return area && area.offsetParent
    ? documentTop(area)
    : documentTop(head) + head.offsetHeight;
}

function decide(app: HTMLElement | null) {
  const head = app?.firstElementChild;
  if (!app || !(head instanceof HTMLElement)) return;
  const root = document.documentElement;
  const room = viewportCssHeight() - contentTop(app, head);
  if (room < MIN_CHART_HEIGHT) root.dataset[ROOT_FLAG] = "true";
  else delete root.dataset[ROOT_FLAG];
}

/**
 * Keeps the startup banner and the app head fixed above a scrolling content
 * row only while that row has room for the smallest chart (240 CSS px);
 * otherwise the whole page scrolls with one scrollbar and the head goes up with
 * it (F06 design README sections 20 item 4, 21 and 32). The room is the window
 * height in the zoomed root's CSS pixels less what sits above the content row,
 * so the app scale counts, not the real window height. It is decided again
 * after every render (a screen opened or closed), on a window resize and when
 * the banner or the head change size. The decision becomes
 * `data-du-page-scroll` on the root, which the frame layouts read.
 */
export function usePageScroll(app: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => decide(app.current));
  useLayoutEffect(() => {
    const element = app.current;
    const head = element?.firstElementChild;
    if (!element || !(head instanceof HTMLElement)) return;
    const update = () => decide(element);
    const observer =
      typeof ResizeObserver === "undefined"
        ? undefined
        : new ResizeObserver(update);
    observer?.observe(head);
    if (element.previousElementSibling)
      observer?.observe(element.previousElementSibling);
    window.addEventListener("resize", update);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", update);
      delete document.documentElement.dataset[ROOT_FLAG];
    };
  }, [app]);
}
