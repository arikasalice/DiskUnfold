import { createContext, useContext, useEffect } from "react";
import type { RefObject } from "react";

/** The smallest chart height; a window too short for it scrolls instead. */
export const MIN_CHART_HEIGHT = 240;
/** Space kept free below the chart. */
export const CHART_BOTTOM_MARGIN = 16;
/** The one-line button row under a chart, until it reports its own height. */
export const DEFAULT_BUTTON_ROW_HEIGHT = 48;

/**
 * The height of the chart box: the visible height of the scroll area, less what
 * sits above the chart inside it (the status line and its gaps), the one-line
 * button row under the chart and the bottom margin, never below the minimum.
 * The aspect ratio is not fixed; the box takes whatever the window leaves.
 */
export function chartHeight(
  areaHeight: number,
  offsetAboveChart: number,
  buttonRowHeight = 0,
) {
  return Math.max(
    MIN_CHART_HEIGHT,
    Math.floor(
      areaHeight - offsetAboveChart - buttonRowHeight - CHART_BOTTOM_MARGIN,
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
  /** `offset`: where the chart starts in the area; `buttonRow`: the height of the row under it. */
  reportOffset: (offset: number, buttonRow: number) => void;
};
export const ChartAreaContext = createContext<ChartArea | null>(null);

/**
 * Reports how far below the top of the scroll area the chart starts and how
 * tall the button row under it is, now and whenever the part above or the row
 * changes size (a status line that wraps, a scrollbar on the row). The owner of
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
    const row = box.querySelector(".du-geometry-buttons");
    const measure = () =>
      area.reportOffset(
        box.getBoundingClientRect().top -
          container.getBoundingClientRect().top +
          container.scrollTop,
        row ? row.getBoundingClientRect().height : 0,
      );
    measure();
    const part = above.current;
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    if (part) observer.observe(part);
    if (row) observer.observe(row);
    return () => observer.disconnect();
  }, [area, chart, above]);
}
