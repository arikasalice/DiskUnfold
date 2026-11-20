import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { HistogramRenderer } from "../src/components/charts/ranked-renderers";
import type {
  GeometryRendererProps,
  HistogramRange,
} from "../src/components/charts/geometry-renderer";
import { ModelRows } from "../src/features/visualization/ModelRows";
import type { VizRow } from "../src/ipc";
import { createProductionVizText, visualizationMessage } from "../src/viz";

afterEach(cleanup);

const LIMIT = 1_000_000;
/** Five intervals; only the second and the fourth hold items. */
const intervals = [
  { name: "0 B–1 KiB", share: 0 },
  { name: "1 KiB–1 MiB", share: 1 },
  { name: "1–2 MiB", share: 0 },
  { name: "2 MiB–1 GiB", share: 0.5 },
  { name: "1 GiB–1 TiB", share: 0 },
];

function props(withRanges: boolean): GeometryRendererProps {
  const count = intervals.length;
  const ranges: HistogramRange[] = intervals.map((interval, index) => ({
    rowRef: `row-${index}`,
    name: interval.name,
    value: interval.share ? "4개" : "0개",
    accessibleLabel: `${interval.name}, 항목 ${interval.share ? 4 : 0}개, 0 B`,
  }));
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
    // The engine draws only intervals that hold something, each in its own slot.
    shapes: intervals.flatMap((interval, index) =>
      interval.share
        ? [
            {
              shapeRef: `shape-${index}`,
              rowRef: `row-${index}`,
              accessibleLabel: interval.name,
              displayName: interval.name,
              displayValue: "4개",
              geometry: {
                primitive: "rect",
                points: [
                  { x: 0, y: Math.round((index / count) * LIMIT) },
                  {
                    x: Math.round(interval.share * LIMIT),
                    y: Math.round(((index + 1) / count) * LIMIT),
                  },
                ],
              },
            },
          ]
        : [],
    ),
    ...(withRanges ? { ranges } : {}),
  };
}

const names = (container: HTMLElement) =>
  Array.from(container.querySelectorAll(".du-geometry-bin-name")).map(
    (node) => node.textContent,
  );

test("F06 README 43: an empty interval between drawn ones keeps its place, name and zero value; outer empty ones are left out", () => {
  const { container } = render(<HistogramRenderer {...props(true)} />);
  expect(names(container)).toEqual(["1 KiB–1 MiB", "1–2 MiB", "2 MiB–1 GiB"]);
  const values = Array.from(
    container.querySelectorAll(".du-geometry-bin-value"),
  ).map((node) => node.textContent);
  expect(values).toEqual(["4개", "0개", "4개"]);
  // Two columns are drawn; the empty interval holds the middle place between them.
  const columns = Array.from(
    container.querySelectorAll(".du-geometry-shape rect"),
  ).map((node) => Number(node.getAttribute("x")));
  expect(columns).toHaveLength(2);
  const empty = screen.getByRole("option", { name: "1–2 MiB, 항목 0개, 0 B" });
  expect(empty.getAttribute("aria-disabled")).toBe("true");
  expect(empty.tabIndex).toBe(-1);
  const left = Number.parseFloat(empty.style.left);
  expect(left).toBeGreaterThan(columns[0]!);
  expect(left).toBeLessThan(columns[1]!);
});

test("F06 README 43: without the table's intervals the drawn columns stand side by side", () => {
  const { container } = render(<HistogramRenderer {...props(false)} />);
  expect(names(container)).toEqual(["1 KiB–1 MiB", "2 MiB–1 GiB"]);
  expect(
    screen
      .getAllByRole("option")
      .every((option) => option.getAttribute("aria-disabled") === null),
  ).toBe(true);
});

test("F06 README 43: an interval with items opens them as a range, an empty one has nothing to open", () => {
  const text = createProductionVizText(
    (key, values) =>
      Object.entries(values ?? {}).reduce(
        (message, [name, value]) => message.replace(`{${name}}`, String(value)),
        visualizationMessage("ko", key),
      ),
    { system: "iec", unit: "auto" },
  );
  const { frame, page } = modelFixture();
  frame.model.representation = {
    ...frame.model.representation,
    format: "histogram",
  };
  const base = page.response.rows[0]!;
  const interval = (name: string, count: string): VizRow => ({
    ...base,
    itemRef: null,
    groupRef: `viz.group.bin.${name}`,
    displayName: name,
    memberCount: { unit: "items", state: "exact", value: count },
  });
  const rows = [interval("0 B–1 KiB", "0"), interval("1 KiB–1 MiB", "4")];
  render(
    <ModelRows
      page={{
        request: page.request,
        response: { ...page.response, rows, totalCount: rows.length },
      }}
      model={frame.model}
      text={text}
      onOpenGroup={() => undefined}
    />,
  );
  const buttons = screen.getAllByRole("button", {
    name: /의 항목 보기$/,
  });
  expect(buttons).toHaveLength(1);
  expect(buttons[0]!.getAttribute("aria-label")).toBe(
    "1 KiB–1 MiB의 항목 보기",
  );
  expect(buttons[0]!.textContent).toBe(
    visualizationMessage("ko", "viewRangeItems"),
  );
  expect(document.body.textContent).not.toContain(
    visualizationMessage("ko", "viewMembers"),
  );
});
