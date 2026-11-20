import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  BarRenderer,
  HistogramRenderer,
} from "../src/components/charts/ranked-renderers";
import type { GeometryRendererProps } from "../src/components/charts/geometry-renderer";

afterEach(cleanup);

const LIMIT = 1_000_000;

/** An engine band: rows divide the height, the length is the share of the axis. */
function band(index: number, count: number, share: number) {
  const top = Math.round((index / count) * LIMIT);
  const bottom = Math.round(((index + 1) / count) * LIMIT);
  return {
    primitive: "rect",
    points: [
      { x: 0, y: top },
      { x: Math.round(share * LIMIT), y: bottom },
    ],
  };
}

function props(
  rows: { name: string; value: string; share: number }[],
): GeometryRendererProps {
  return {
    modelRef: "model-1",
    resultRevision: "revision-1",
    label: "Space distribution",
    selectedText: "selected",
    fallbackNotice: "Graph unavailable",
    missingFallbackNotice: "Table unavailable",
    width: 500,
    height: 300,
    viewport: { left: 0, top: 0, right: LIMIT, bottom: LIMIT },
    selectedRowRef: null,
    onSelect: vi.fn(),
    fallback: {
      modelRef: "model-1",
      resultRevision: "revision-1",
      content: null,
    },
    relations: [],
    shapes: rows.map((row, index) => ({
      shapeRef: `shape-${index}`,
      rowRef: `row-${index}`,
      accessibleLabel: `${row.name}, ${row.value}`,
      displayName: row.name,
      displayValue: row.value,
      geometry: band(index, rows.length, row.share),
    })),
  };
}

const widthOf = (option: Element) =>
  Number.parseFloat((option as HTMLElement).style.width);

test("F06 README 12: bars are 24 px rows in model order with the name on the left and the value after the bar (#240)", () => {
  const { container } = render(
    <BarRenderer
      {...props([
        { name: "Videos", value: "128 MiB", share: 1 },
        { name: "Photos", value: "64 MiB", share: 0.5 },
        { name: "Other 2", value: "32 MiB", share: 0.25 },
      ])}
    />,
  );
  const svg = container.querySelector("svg")!;
  // Three rows of 24 with 12 between, and 4 above and below.
  expect(svg.getAttribute("height")).toBe(String(4 + 3 * 24 + 2 * 12 + 4));
  const names = Array.from(
    container.querySelectorAll(".du-geometry-bar-name"),
  ).map((node) => node.textContent);
  expect(names).toEqual(["Videos", "Photos", "Other 2"]);
  const values = Array.from(
    container.querySelectorAll(".du-geometry-bar-value"),
  ).map((node) => node.textContent);
  expect(values).toEqual(["128 MiB", "64 MiB", "32 MiB"]);
  // Names and values are outside the bars; nothing is written inside them.
  expect(container.querySelector(".du-geometry-label")).toBeNull();
  const options = screen.getAllByRole("option");
  expect(options.map((option) => option.getAttribute("aria-label"))).toEqual([
    "Videos, 128 MiB",
    "Photos, 64 MiB",
    "Other 2, 32 MiB",
  ]);
  // Lengths keep the engine's shares of one axis from zero.
  const [first, second, third] = options.map(widthOf);
  expect(Math.round((second! / first!) * 100)).toBe(50);
  expect(Math.round((third! / first!) * 100)).toBe(25);
  // The name column takes at most 40% of the width.
  expect(
    Number.parseFloat((options[0] as HTMLElement).style.left),
  ).toBeLessThanOrEqual(0.4 * 500 + 8 + 1);
});

test("F06 README 12: a long bar name keeps its start and end within the name column", () => {
  const { container } = render(
    <BarRenderer
      {...props([
        {
          name: "a-very-long-folder-name-that-does-not-fit-the-column.bin",
          value: "1 MiB",
          share: 1,
        },
      ])}
      width={200}
    />,
  );
  const name = container.querySelector(".du-geometry-bar-name")!.textContent!;
  expect(name).toContain("…");
  expect(name.startsWith("a-ver")).toBe(true);
  expect(name.endsWith(".bin")).toBe(true);
});

test("F06 README 12: a histogram draws one column per bin from a zero baseline with its name below (#240)", () => {
  const { container } = render(
    <HistogramRenderer
      {...props([
        { name: "0–1 MiB", value: "3 items", share: 0.75 },
        { name: "1–10 MiB", value: "4 items", share: 1 },
      ])}
    />,
  );
  expect(
    Array.from(container.querySelectorAll(".du-geometry-bin-name")).map(
      (node) => node.textContent,
    ),
  ).toEqual(["0–1 MiB", "1–10 MiB"]);
  expect(container.querySelector(".du-geometry-baseline")).toBeTruthy();
  const options = screen.getAllByRole("option");
  const heights = options.map((option) =>
    Number.parseFloat((option as HTMLElement).style.height),
  );
  expect(Math.round((heights[0]! / heights[1]!) * 100)).toBe(75);
  // Columns stand side by side in bin order.
  const lefts = options.map((option) =>
    Number.parseFloat((option as HTMLElement).style.left),
  );
  expect(lefts[0]!).toBeLessThan(lefts[1]!);
});
