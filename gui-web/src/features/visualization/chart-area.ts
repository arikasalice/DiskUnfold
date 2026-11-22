import { createContext, useContext, useEffect } from "react";
import type { RefObject } from "react";

/** The smallest chart height; a window too short for it scrolls instead. */
export const MIN_CHART_HEIGHT = 240;
/** Space kept free below the chart. */
export const CHART_BOTTOM_MARGIN = 16;
/**
 * Room kept on each side of the chart for the focus line of a shape at its
 * edge: a 1 px gap and a 2 px line (F06 README 4.7, 35). The chart frame has
 * this margin; the chart is requested that much narrower.
 */
export const CHART_FOCUS_ROOM = 3;
/**
 * Room kept above the chart from the first request: the status line and one
 * line of notes under it (undrawn items, unread folders), 94 CSS px at 100%.
 * Those lines arrive with the first model; were the chart sized without them,
 * their arrival would ask for the whole chart again, which on a large result
 * keeps "Calculating the chart" over a drawn chart for as long as the first
 * calculation took (#365).
 */
export const CHART_OFFSET_RESERVE = 94;

/**
 * The height of the chart box: the visible height of the scroll area, less what
 * sits above the chart inside it (the status line and its gaps, at least
 * CHART_OFFSET_RESERVE) and the bottom margin, never below the minimum.
 * The aspect ratio is not fixed; the box takes whatever the window leaves.
 */
export function chartHeight(areaHeight: number, offsetAboveChart: number) {
  return Math.max(
    MIN_CHART_HEIGHT,
    Math.floor(
      areaHeight -
        Math.max(offsetAboveChart, CHART_OFFSET_RESERVE) -
        CHART_BOTTOM_MARGIN,
    ),
  );
}

/**
 * The size of the part of `box` a chart can use: its border box less the
 * scrollbars it shows. A chart as wide as the border box would run under a
 * vertical scrollbar, and one as tall would hide behind a horizontal one.
 */
export function usableSize(
  rect: { width: number; height: number },
  box: {
    offsetWidth: number;
    clientWidth: number;
    offsetHeight: number;
    clientHeight: number;
  },
) {
  return {
    width: rect.width - Math.max(0, box.offsetWidth - box.clientWidth),
    height: rect.height - Math.max(0, box.offsetHeight - box.clientHeight),
  };
}

/** The scroll area that holds the chart and the way the chart reports where it starts in it. */
export type ChartArea = {
  element: RefObject<HTMLElement | null>;
  /** `offset`: where the chart starts in the area. */
  reportOffset: (offset: number) => void;
};
export const ChartAreaContext = createContext<ChartArea | null>(null);

/**
 * Reports how far below the top of the scroll area the chart starts, now and
 * whenever the part above changes size (a status line that wraps). The owner of
 * the area sizes the chart request from it.
 */
export function useChartOffset(
  chart: RefObject<HTMLElement | null>,
  above: RefObject<HTMLElement | null>,
) {
  const area = useContext(ChartAreaContext);
  useEffect(() => {
    const box = chart.current;
    const container = area?.element.current;
    if (!area || !box || !container) return;
    const measure = () =>
      area.reportOffset(
        box.getBoundingClientRect().top -
          container.getBoundingClientRect().top +
          container.scrollTop,
      );
    measure();
    const part = above.current;
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    if (part) observer.observe(part);
    return () => observer.disconnect();
  }, [area, chart, above]);
}
