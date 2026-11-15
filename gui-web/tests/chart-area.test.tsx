import { cleanup, render } from "@testing-library/react";
import { createRef, useRef } from "react";
import { afterEach, expect, test, vi } from "vitest";
import {
  CHART_BOTTOM_MARGIN,
  ChartAreaContext,
  MIN_CHART_HEIGHT,
  chartHeight,
  useChartOffset,
} from "../src/features/visualization/chart-area";

afterEach(cleanup);

test.each([
  // 1280x800 window: the scroll area leaves 560 px, the status line starts 44 px down.
  [560, 44, 560 - 44 - CHART_BOTTOM_MARGIN],
  [900.9, 40, 844],
  [300, 44, MIN_CHART_HEIGHT],
  [0, 0, MIN_CHART_HEIGHT],
  [-50, 10, MIN_CHART_HEIGHT],
])(
  "an area of %s px with the chart starting %s px down gets a chart %s px high",
  (area, offset, expected) => {
    expect(chartHeight(area, offset)).toBe(expected);
  },
);

test("the chart height never drops below the minimum and keeps the bottom margin", () => {
  expect(chartHeight(10_000, 0)).toBe(10_000 - CHART_BOTTOM_MARGIN);
  expect(MIN_CHART_HEIGHT).toBe(240);
  expect(CHART_BOTTOM_MARGIN).toBe(16);
});

test.each([
  // 1280x800: 560 px area, chart starts 44 px down, a 48 px button row under it.
  [560, 44, 48, 560 - 44 - 48 - CHART_BOTTOM_MARGIN],
  [560, 44, 0, 560 - 44 - CHART_BOTTOM_MARGIN],
  [320, 44, 48, MIN_CHART_HEIGHT],
])(
  "an area of %s px with the chart %s px down and a %s px button row gets a chart %s px high",
  (area, offset, row, expected) => {
    expect(chartHeight(area, offset, row)).toBe(expected);
  },
);

test("the chart reports where it starts in the scroll area", () => {
  const report = vi.fn();
  const areaRef = createRef<HTMLDivElement>();
  const rect = (top: number) => ({ top }) as DOMRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this.dataset.role === "chart") return rect(150);
      if (this.dataset.role === "row") return { top: 0, height: 46 } as DOMRect;
      if (this.dataset.role === "area") return rect(100);
      return rect(0);
    },
  );
  function Chart() {
    const chart = useRef<HTMLDivElement>(null);
    const above = useRef<HTMLDivElement>(null);
    useChartOffset(chart, above);
    return (
      <>
        <div ref={above} />
        <div ref={chart} data-role="chart">
          <div className="du-geometry-buttons" data-role="row" />
        </div>
      </>
    );
  }
  render(
    <ChartAreaContext.Provider
      value={{ element: areaRef, reportOffset: report }}
    >
      <div ref={areaRef} data-role="area">
        <Chart />
      </div>
    </ChartAreaContext.Provider>,
  );
  // 150 - 100, plus the area's own scroll offset (0).
  expect(report).toHaveBeenCalledWith(50, 46);
  vi.restoreAllMocks();
});
