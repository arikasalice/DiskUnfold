import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { createChartRenderers } from "../src/components/chart-renderers";
import { GeometryRenderer } from "../src/components/geometry-renderer";
import type { GeometryRendererProps } from "../src/components/geometry-renderer";

afterEach(cleanup);
const rows = (
  <table>
    <caption>Same model rows</caption>
    <tbody>
      <tr>
        <td>Original row A</td>
        <td>10 bytes</td>
      </tr>
      <tr>
        <td>Original row B</td>
        <td>unknown</td>
      </tr>
    </tbody>
  </table>
);
function props(): GeometryRendererProps {
  return {
    modelRef: "model-1",
    resultRevision: "revision-1",
    label: "Capacity graph",
    selectedText: "selected",
    fallbackNotice: "Graph unavailable",
    missingFallbackNotice: "Table unavailable",
    width: 200,
    height: 100,
    viewport: { left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 },
    selectedRowRef: null,
    onSelect: vi.fn(),
    fallback: {
      modelRef: "model-1",
      resultRevision: "revision-1",
      content: rows,
    },
    shapes: [
      {
        shapeRef: "one",
        rowRef: "row-a",
        accessibleLabel: "Item A, 10 bytes",
        geometry: {
          primitive: "rect",
          points: [
            { x: 0, y: 0 },
            { x: 500_000, y: 1_000_000 },
          ],
        },
      },
      {
        shapeRef: "two",
        rowRef: "row-b",
        accessibleLabel: "Item B, unknown",
        geometry: {
          primitive: "point",
          points: [{ x: 1_000_000, y: 500_000 }],
        },
      },
      {
        shapeRef: "three",
        rowRef: "row-c",
        accessibleLabel: "Line",
        geometry: {
          primitive: "polyline",
          points: [
            { x: 0, y: 0 },
            { x: 1_000_000, y: 1_000_000 },
          ],
        },
      },
      {
        shapeRef: "four",
        rowRef: "row-d",
        accessibleLabel: "Area",
        geometry: {
          primitive: "polygon",
          points: [
            { x: 0, y: 0 },
            { x: 500_000, y: 0 },
            { x: 0, y: 1_000_000 },
          ],
        },
      },
    ],
    relations: [
      {
        fromShapeRef: "one",
        toShapeRef: "two",
        geometry: {
          primitive: "polyline",
          points: [
            { x: 0, y: 500_000 },
            { x: 1_000_000, y: 500_000 },
          ],
        },
      },
    ],
  };
}

test("renders all common geometry and leaves same-model rows readable", async () => {
  const { container } = render(<GeometryRenderer {...props()} />);
  expect(
    container.querySelector("svg rect[x='0']")?.getAttribute("width"),
  ).toBe("100");
  expect(container.querySelector("circle")?.getAttribute("cx")).toBe("200");
  expect(container.querySelectorAll("polyline")).toHaveLength(2);
  expect(container.querySelectorAll("polygon")).toHaveLength(1);
  expect(
    screen.getByRole("table", { name: "Same model rows" }).textContent,
  ).toContain("Original row A");
  expect(screen.getByText("unknown")).toBeTruthy();
  expect((await axe.run(container)).violations).toEqual([]);
});

test("future format geometry uses the same rendering, clipping and selection", () => {
  const resolve = createChartRenderers([]);
  const Renderer = resolve("fixture.future");
  function Harness() {
    const [selected, setSelected] = useState<string | null>(null);
    const input = props();
    input.viewport = { left: 0, top: 0, right: 500_000, bottom: 1_000_000 };
    return (
      <Renderer {...input} selectedRowRef={selected} onSelect={setSelected} />
    );
  }
  const { container } = render(<Harness />);
  expect(container.querySelector("svg g[clip-path]")).toBeTruthy();
  const a = screen.getByRole("button", { name: "Item A, 10 bytes" });
  a.focus();
  fireEvent.keyDown(a, { key: "ArrowRight" });
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Line" }),
  );
  fireEvent.click(a);
  expect(
    screen.getByRole("button", { name: "Item A, 10 bytes, selected" }),
  ).toBeTruthy();
  expect(screen.getByRole("table", { name: "Same model rows" })).toBeTruthy();
});

test("a dedicated display can be registered without changing common fallback", () => {
  function DedicatedRenderer(input: GeometryRendererProps) {
    return <GeometryRenderer {...input} label="Dedicated graph" />;
  }
  const registrations = [["fixture.future", DedicatedRenderer]] as const;
  const resolve = createChartRenderers(registrations);
  expect(resolve("fixture.future")).toBe(DedicatedRenderer);
  expect(resolve("fixture.other")).toBe(GeometryRenderer);
  expect(() =>
    createChartRenderers([...registrations, ...registrations]),
  ).toThrow("Invalid chart renderer registration: fixture.future");
});

test("unknown or malformed geometry and invalid viewport render no partial graph", () => {
  const Renderer = createChartRenderers([])("fixture.future");
  for (const mutate of [
    (p: GeometryRendererProps) => {
      p.shapes = p.shapes.map((shape, i) =>
        i === 1
          ? {
              ...shape,
              geometry: { ...shape.geometry, primitive: "future.arc" },
            }
          : shape,
      );
    },
    (p: GeometryRendererProps) => {
      p.shapes = p.shapes.map((shape, i) =>
        i === 1
          ? {
              ...shape,
              geometry: { ...shape.geometry, points: [{ x: -1, y: 0 }] },
            }
          : shape,
      );
    },
    (p: GeometryRendererProps) => {
      p.relations[0]!.toShapeRef = "missing";
    },
    (p: GeometryRendererProps) => {
      p.viewport.right = 0;
    },
  ]) {
    const input = props();
    mutate(input);
    const result = render(<Renderer {...input} />);
    expect(result.container.querySelector("svg.du-geometry")).toBeNull();
    expect(screen.getByText("Graph unavailable")).toBeTruthy();
    expect(screen.getByText("Original row A")).toBeTruthy();
    result.unmount();
  }
});

test("does not display fallback from a different model or result revision", () => {
  const input = props();
  input.fallback.resultRevision = "other";
  const { container } = render(<GeometryRenderer {...input} />);
  expect(container.querySelector("svg.du-geometry")).toBeNull();
  expect(screen.getByText("Table unavailable")).toBeTruthy();
  expect(screen.queryByText("Original row A")).toBeNull();
});

test("keeps subpixel and boundary-only rectangles in the table without invisible actions", () => {
  const input = props();
  input.viewport = { left: 0, top: 0, right: 500_000, bottom: 1_000_000 };
  const rect = input.shapes[0]!;
  input.shapes = [
    {
      ...rect,
      accessibleLabel: "Subpixel rectangle",
      geometry: {
        primitive: "rect",
        points: [
          { x: 0, y: 0 },
          { x: 1, y: 100_000 },
        ],
      },
    },
    {
      ...rect,
      shapeRef: "touching",
      accessibleLabel: "Touching rectangle",
      geometry: {
        primitive: "rect",
        points: [
          { x: 500_000, y: 0 },
          { x: 600_000, y: 100_000 },
        ],
      },
    },
  ];
  input.relations = [];
  const { container } = render(<GeometryRenderer {...input} />);
  expect(container.querySelector(".du-geometry-shape")).toBeNull();
  expect(screen.queryAllByRole("button")).toHaveLength(0);
  expect(screen.getByText("Original row A")).toBeTruthy();
});
