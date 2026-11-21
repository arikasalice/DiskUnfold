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
  ).toBeLessThanOrEqual(0.4 * 500 + 12 + 1);
  // Short names keep it narrow: the bars start 12 px after the longest name (README 43).
  expect(
    Number.parseFloat((options[0] as HTMLElement).style.left),
  ).toBeLessThan(100);
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
  // A tall column carries no inner label; its name is below it (#240).
  expect(container.querySelector(".du-geometry-label")).toBeNull();
  expect(container.querySelector("svg")!.getAttribute("data-variant")).toBe(
    "ranked",
  );
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

test("#318: a folder's bar name starts with the folder icon; a file's has none", () => {
  const base = props([
    { name: "docs", value: "4 MiB", share: 1 },
    { name: "clip.mp4", value: "6 MiB", share: 0.6 },
  ]);
  const shapes = base.shapes.map((shape, index) =>
    index === 0 ? { ...shape, folder: true } : shape,
  );
  const { container } = render(<BarRenderer {...base} shapes={shapes} />);
  const icons = container.querySelectorAll(".du-chart-icon");
  expect(icons).toHaveLength(1);
  const names = Array.from(container.querySelectorAll(".du-geometry-bar-name"));
  // The folder's name moves right of its icon; the file's stays at the edge.
  expect(Number(names[0]!.getAttribute("x"))).toBe(4 + 14 + 4);
  expect(Number(names[1]!.getAttribute("x"))).toBe(4);
  expect(icons[0]!.getAttribute("aria-hidden")).toBe("true");
});

test("F06 README 47: the axis ends at the largest single bar and a larger Other is drawn to the end with a break", () => {
  const base = props([
    { name: "big.iso", value: "10 MiB", share: 0.1 },
    { name: "small.txt", value: "5 MiB", share: 0.05 },
    { name: "Other 80", value: "100 MiB", share: 1 },
  ]);
  const shapes = base.shapes.map((shape, index) =>
    index === 2 ? { ...shape, group: true } : shape,
  );
  const { container } = render(
    <BarRenderer
      {...base}
      shapes={shapes}
      drawnShorterNote="Drawn shorter than scale"
      notToScaleMark="not to scale"
    />,
  );
  const widths = Array.from(
    container.querySelectorAll(".du-geometry-shape rect"),
  ).map((rect) => Number(rect.getAttribute("width")));
  // The largest single bar spans the plot; the half-size one half of it; Other
  // spans it too, past its proportion, and carries the break.
  expect(widths[0]).toBe(widths[2]);
  expect(Math.abs(widths[1]! * 2 - widths[0]!)).toBeLessThanOrEqual(2);
  expect(container.querySelectorAll(".du-geometry-bar-break")).toHaveLength(2);
  // Forced colours draw each gap as a line instead (README 47.2).
  expect(
    container.querySelectorAll(".du-geometry-bar-break-line"),
  ).toHaveLength(2);
  const other = container.querySelector('[aria-label="Other 80, 100 MiB"]');
  // The info card shows it on hover and focus; assistive technology hears it too.
  expect(other?.getAttribute("aria-description")).toBe(
    "Drawn shorter than scale",
  );
  // Its value says it is not to scale; the single bars' values do not.
  const notes = container.querySelectorAll(".du-geometry-bar-note");
  expect(notes).toHaveLength(1);
  expect(notes[0]!.textContent).toBe(" · not to scale");
});

test("F06 README 47: an Other no larger than the largest single bar is not broken", () => {
  const base = props([
    { name: "big.iso", value: "10 MiB", share: 1 },
    { name: "Other 3", value: "4 MiB", share: 0.4 },
  ]);
  const shapes = base.shapes.map((shape, index) =>
    index === 1 ? { ...shape, group: true } : shape,
  );
  const { container } = render(
    <BarRenderer
      {...base}
      shapes={shapes}
      drawnShorterNote="Drawn shorter than scale"
      notToScaleMark="not to scale"
    />,
  );
  expect(container.querySelectorAll(".du-geometry-bar-break")).toHaveLength(0);
  expect(container.querySelector("[title]")).toBeNull();
  expect(container.querySelector(".du-geometry-bar-note")).toBeNull();
});
