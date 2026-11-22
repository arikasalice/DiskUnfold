import { cleanup, render } from "@testing-library/react";
import { createRef, useRef } from "react";
import { afterEach, expect, test, vi } from "vitest";
import {
  CHART_BOTTOM_MARGIN,
  CHART_OFFSET_RESERVE,
  ChartAreaContext,
  MIN_CHART_HEIGHT,
  chartHeight,
  useChartOffset,
} from "../src/features/visualization/chart-area";

afterEach(cleanup);

test.each([
  // 1280x800 window: the scroll area leaves 560 px; the status line and a line
  // of notes take the reserve, a wrapped line more.
  [560, 44, 560 - CHART_OFFSET_RESERVE - CHART_BOTTOM_MARGIN],
  [560, 130, 560 - 130 - CHART_BOTTOM_MARGIN],
  [900.9, 120, 764],
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
  expect(chartHeight(10_000, 0)).toBe(
    10_000 - CHART_OFFSET_RESERVE - CHART_BOTTOM_MARGIN,
  );
  expect(MIN_CHART_HEIGHT).toBe(240);
  expect(CHART_BOTTOM_MARGIN).toBe(16);
});

test("the lines that arrive with the first model do not change the chart height (#365)", () => {
  // The first request assumes the reserve; the status line alone (40) or with
  // a line of notes (94) keeps the same height, so no second chart is asked for.
  const first = chartHeight(789, CHART_OFFSET_RESERVE);
  expect(chartHeight(789, 40)).toBe(first);
  expect(chartHeight(789, 94)).toBe(first);
});

test("the chart reports where it starts in the scroll area", () => {
  const report = vi.fn();
  const areaRef = createRef<HTMLDivElement>();
  const rect = (top: number) => ({ top }) as DOMRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this.dataset.role === "chart") return rect(150);
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
        <div ref={chart} data-role="chart" />
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
  expect(report).toHaveBeenCalledWith(50);
  vi.restoreAllMocks();
});
