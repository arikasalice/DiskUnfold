import { createContext, useContext, useEffect } from "react";
import type { RefObject } from "react";

/** The smallest chart height; a window too short for it scrolls instead. */
export const MIN_CHART_HEIGHT = 240;
/** Space kept free below the chart. */
export const CHART_BOTTOM_MARGIN = 16;

/**
 * The height of the chart box: the visible height of the scroll area, less what
 * sits above the chart inside it (the status line and its gaps) and the bottom
 * margin, never below the minimum. The aspect ratio is not fixed; the box
 * takes whatever the window leaves.
 */
export function chartHeight(areaHeight: number, offsetAboveChart: number) {
  return Math.max(
    MIN_CHART_HEIGHT,
    Math.floor(areaHeight - offsetAboveChart - CHART_BOTTOM_MARGIN),
  );
}

/** The scroll area that holds the chart and the way the chart reports where it starts in it. */
export type ChartArea = {
  element: RefObject<HTMLElement | null>;
  reportOffset: (offset: number) => void;
};
export const ChartAreaContext = createContext<ChartArea | null>(null);

/**
 * Reports how far below the top of the scroll area the chart starts, now and
 * whenever the part above it changes size (a status line that wraps). The owner
 * of the area sizes the chart request from it.
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
    if (!part || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(part);
    return () => observer.disconnect();
  }, [area, chart, above]);
}
