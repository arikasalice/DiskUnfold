import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import axe from "axe-core";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { createChartRenderers } from "../src/components/charts/chart-renderers";
import { GeometryRenderer } from "../src/components/charts/geometry-renderer";
import type { GeometryRendererProps } from "../src/components/charts/geometry-renderer";

afterEach(cleanup);

test("F06 README 34: the shapes are the options of one list with one Tab stop, and no button row", () => {
  const { container } = render(<GeometryRenderer {...props()} />);
  const list = screen.getByRole("listbox", { name: "Capacity graph" });
  const options = Array.from(list.querySelectorAll('[role="option"]'));
  expect(options.map((option) => option.getAttribute("aria-label"))).toEqual([
    "Item A, 10 bytes",
    "Item B, unknown",
    "Line",
    "Area",
  ]);
  expect(options.map((option) => option.getAttribute("tabindex"))).toEqual([
    "0",
    "-1",
    "-1",
    "-1",
  ]);
  expect(screen.queryAllByRole("button")).toHaveLength(0);
  expect(container.querySelector(".du-geometry-buttons")).toBeNull();
  // The last focused shape keeps the Tab stop.
  act(() => (options[1] as HTMLElement).focus());
  expect(options.map((option) => option.getAttribute("tabindex"))).toEqual([
    "-1",
    "0",
    "-1",
    "-1",
  ]);
});

test("F06 README 10, 22, 34: arrows go to the nearest shape on screen and stop at the edge; Home, End, Space, Enter and Esc", () => {
  const input = props();
  const onActivate = vi.fn();
  const onClearSelection = vi.fn();
  const rect = (left: number, top: number, right: number, bottom: number) => ({
    primitive: "rect",
    points: [
      { x: left, y: top },
      { x: right, y: bottom },
    ],
  });
  // A 2 x 2 grid: left column A over C, right column B over D.
  input.shapes = [
    {
      shapeRef: "a",
      rowRef: "row-a",
      accessibleLabel: "A",
      geometry: rect(0, 0, 500_000, 500_000),
    },
    {
      shapeRef: "b",
      rowRef: "row-b",
      accessibleLabel: "B",
      geometry: rect(500_000, 0, 1_000_000, 500_000),
    },
    {
      shapeRef: "c",
      rowRef: "row-c",
      accessibleLabel: "C",
      geometry: rect(0, 500_000, 500_000, 1_000_000),
      container: true,
    },
    {
      shapeRef: "d",
      rowRef: "row-d",
      accessibleLabel: "D",
      geometry: rect(500_000, 500_000, 1_000_000, 1_000_000),
    },
  ];
  input.relations = [];
  const { rerender } = render(
    <GeometryRenderer
      {...input}
      onActivate={onActivate}
      onClearSelection={onClearSelection}
    />,
  );
  const option = (name: string) => screen.getByRole("option", { name });
  act(() => option("A").focus());
  fireEvent.keyDown(option("A"), { key: "ArrowDown" });
  expect(document.activeElement).toBe(option("C"));
  fireEvent.keyDown(option("C"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(option("D"));
  // Nothing lies further right or down: the focus stays and does not wrap.
  fireEvent.keyDown(option("D"), { key: "ArrowRight" });
  fireEvent.keyDown(option("D"), { key: "ArrowDown" });
  expect(document.activeElement).toBe(option("D"));
  fireEvent.keyDown(option("D"), { key: "Home" });
  expect(document.activeElement).toBe(option("A"));
  fireEvent.keyDown(option("A"), { key: "End" });
  expect(document.activeElement).toBe(option("D"));
  fireEvent.keyDown(option("D"), { key: " " });
  expect(input.onSelect).toHaveBeenLastCalledWith("row-d", "d");
  fireEvent.keyDown(option("C"), { key: "Enter" });
  expect(onActivate).toHaveBeenLastCalledWith("row-c", "c", true);
  fireEvent.keyDown(option("D"), { key: "Enter" });
  expect(onActivate).toHaveBeenLastCalledWith("row-d", "d", false);
  // Esc unselects only while a shape is selected.
  fireEvent.keyDown(option("D"), { key: "Escape" });
  expect(onClearSelection).not.toHaveBeenCalled();
  rerender(
    <GeometryRenderer
      {...input}
      selectedRowRef="row-d"
      onActivate={onActivate}
      onClearSelection={onClearSelection}
    />,
  );
  expect(option("D").getAttribute("aria-selected")).toBe("true");
  expect(option("A").getAttribute("aria-selected")).toBe("false");
  fireEvent.keyDown(option("D"), { key: "Escape" });
  expect(onClearSelection).toHaveBeenCalledTimes(1);
});

test("F06 README 35: a folder stands where its header band is; the arrow goes to the file beside a file and up to the header", () => {
  const input = props();
  const rect = (left: number, top: number, right: number, bottom: number) => ({
    primitive: "rect",
    points: [
      { x: left, y: top },
      { x: right, y: bottom },
    ],
  });
  input.shapes = [
    {
      shapeRef: "folder",
      rowRef: "row-folder",
      accessibleLabel: "Photos",
      geometry: rect(0, 0, 1_000_000, 1_000_000),
      container: true,
    },
    {
      shapeRef: "left",
      rowRef: "row-left",
      accessibleLabel: "album-a",
      geometry: rect(0, 200_000, 400_000, 1_000_000),
    },
    {
      shapeRef: "right",
      rowRef: "row-right",
      accessibleLabel: "album-b",
      geometry: rect(400_000, 200_000, 1_000_000, 1_000_000),
    },
  ];
  input.relations = [];
  render(<GeometryRenderer {...input} />);
  const option = (name: string) => screen.getByRole("option", { name });
  act(() => option("album-b").focus());
  fireEvent.keyDown(option("album-b"), { key: "ArrowLeft" });
  expect(document.activeElement).toBe(option("album-a"));
  fireEvent.keyDown(option("album-a"), { key: "ArrowUp" });
  expect(document.activeElement).toBe(option("Photos"));
  // Down from the header: a shape of the first row inside, the one nearest its centre.
  fireEvent.keyDown(option("Photos"), { key: "ArrowDown" });
  expect(document.activeElement).toBe(option("album-b"));
});

test("F06 README 10: a wide shape's right neighbour is the shape level with it, not one nearer below", () => {
  const input = props();
  const rect = (left: number, top: number, right: number, bottom: number) => ({
    primitive: "rect",
    points: [
      { x: left, y: top },
      { x: right, y: bottom },
    ],
  });
  // A wide shape on top left, a tall one on the right, a small one under the wide one.
  input.shapes = [
    {
      shapeRef: "wide",
      rowRef: "row-wide",
      accessibleLabel: "Videos",
      geometry: rect(0, 0, 900_000, 500_000),
    },
    {
      shapeRef: "tall",
      rowRef: "row-tall",
      accessibleLabel: "notes",
      geometry: rect(900_000, 0, 1_000_000, 1_000_000),
    },
    {
      shapeRef: "below",
      rowRef: "row-below",
      accessibleLabel: "album",
      geometry: rect(500_000, 500_000, 900_000, 1_000_000),
    },
  ];
  input.relations = [];
  render(<GeometryRenderer {...input} />);
  const option = (name: string) => screen.getByRole("option", { name });
  act(() => option("Videos").focus());
  fireEvent.keyDown(option("Videos"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(option("notes"));
});

test("#186: among shapes as near, an arrow back returns to the shape focused before", () => {
  const input = props();
  const rect = (left: number, top: number, right: number, bottom: number) => ({
    primitive: "rect",
    points: [
      { x: left, y: top },
      { x: right, y: bottom },
    ],
  });
  // Two shapes stacked on the left, one tall shape on the right beside both.
  input.shapes = [
    {
      shapeRef: "upper",
      rowRef: "row-upper",
      accessibleLabel: "upper",
      geometry: rect(0, 0, 500_000, 500_000),
    },
    {
      shapeRef: "lower",
      rowRef: "row-lower",
      accessibleLabel: "lower",
      geometry: rect(0, 500_000, 500_000, 1_000_000),
    },
    {
      shapeRef: "tall",
      rowRef: "row-tall",
      accessibleLabel: "tall",
      geometry: rect(500_000, 0, 1_000_000, 1_000_000),
    },
  ];
  input.relations = [];
  render(<GeometryRenderer {...input} />);
  const option = (name: string) => screen.getByRole("option", { name });
  for (const from of ["lower", "upper"]) {
    act(() => option(from).focus());
    fireEvent.keyDown(option(from), { key: "ArrowRight" });
    expect(document.activeElement).toBe(option("tall"));
    fireEvent.keyDown(option("tall"), { key: "ArrowLeft" });
    expect(document.activeElement).toBe(option(from));
  }
});

test("#186: without a shape focused before, the longest shared edge wins, then the upper, then the left shape", () => {
  const input = props();
  const rect = (left: number, top: number, right: number, bottom: number) => ({
    primitive: "rect",
    points: [
      { x: left, y: top },
      { x: right, y: bottom },
    ],
  });
  input.shapes = [
    // On the left of "start": a short shape on top and a longer one below.
    {
      shapeRef: "short",
      rowRef: "row-short",
      accessibleLabel: "short",
      geometry: rect(0, 0, 500_000, 300_000),
    },
    {
      shapeRef: "long",
      rowRef: "row-long",
      accessibleLabel: "long",
      geometry: rect(0, 300_000, 500_000, 1_000_000),
    },
    {
      shapeRef: "start",
      rowRef: "row-start",
      accessibleLabel: "start",
      geometry: rect(500_000, 0, 750_000, 1_000_000),
    },
    // On the right of "start": two halves sharing equal edges.
    {
      shapeRef: "top-half",
      rowRef: "row-top",
      accessibleLabel: "top-half",
      geometry: rect(750_000, 0, 1_000_000, 500_000),
    },
    {
      shapeRef: "bottom-half",
      rowRef: "row-bottom",
      accessibleLabel: "bottom-half",
      geometry: rect(750_000, 500_000, 1_000_000, 1_000_000),
    },
  ];
  input.relations = [];
  render(<GeometryRenderer {...input} />);
  const option = (name: string) => screen.getByRole("option", { name });
  act(() => option("start").focus());
  fireEvent.keyDown(option("start"), { key: "ArrowLeft" });
  expect(document.activeElement).toBe(option("long"));
  act(() => option("start").focus());
  fireEvent.keyDown(option("start"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(option("top-half"));
});

test("F06 README 4.7: the selection is a solid band with a check", () => {
  const input = props();
  input.width = 400;
  input.height = 200;
  const { container } = render(
    <GeometryRenderer {...input} selectedRowRef="row-a" />,
  );
  expect(container.querySelector(".du-geometry-selection")).toBeTruthy();
  expect(container.querySelector(".du-geometry-check")).toBeTruthy();
});

test("F06 README 22, 34: a focused shape that leaves the chart passes the focus to the shape at its place, else the last one, else the caller", () => {
  const input = props();
  input.relations = [];
  const moved = vi.fn();
  const missing = vi.fn();
  const { rerender } = render(
    <GeometryRenderer
      {...input}
      onFocusMoved={moved}
      onFocusTargetMissing={missing}
    />,
  );
  act(() => screen.getByRole("option", { name: "Line" }).focus());
  // The next result lacks "Line" (third); the shape now third takes the focus.
  const next = {
    ...input,
    modelRef: "next",
    fallback: { ...input.fallback, modelRef: "next" },
    shapes: input.shapes.filter((shape) => shape.shapeRef !== "three"),
  };
  rerender(
    <GeometryRenderer
      {...next}
      onFocusMoved={moved}
      onFocusTargetMissing={missing}
    />,
  );
  expect(document.activeElement).toBe(
    screen.getByRole("option", { name: "Area" }),
  );
  expect(moved).toHaveBeenCalledTimes(1);
  // A result with fewer shapes: the last one.
  const last = {
    ...next,
    modelRef: "last",
    fallback: { ...next.fallback, modelRef: "last" },
    shapes: next.shapes.slice(0, 1),
  };
  rerender(
    <GeometryRenderer
      {...last}
      onFocusMoved={moved}
      onFocusTargetMissing={missing}
    />,
  );
  expect(document.activeElement).toBe(
    screen.getByRole("option", { name: "Item A, 10 bytes" }),
  );
  expect(moved).toHaveBeenCalledTimes(2);
  // No shape left: the caller takes the focus.
  rerender(
    <GeometryRenderer
      {...last}
      modelRef="empty"
      fallback={{ ...last.fallback, modelRef: "empty" }}
      shapes={[]}
      onFocusMoved={moved}
      onFocusTargetMissing={missing}
    />,
  );
  expect(missing).toHaveBeenCalledTimes(1);
});

test("F06 FR12.HD1.gui-web.DD1: a lower-bound rectangle has a dashed right and bottom edge, an exact one has none", () => {
  const base = props();
  const rect = (left: number, right: number) => ({
    primitive: "rect",
    points: [
      { x: left, y: 0 },
      { x: right, y: 1_000_000 },
    ],
  });
  const { container } = render(
    <GeometryRenderer
      {...base}
      width={200}
      height={100}
      relations={[]}
      shapes={[
        {
          shapeRef: "lower-leaf",
          rowRef: "row-lower-leaf",
          accessibleLabel: "At least 10 bytes",
          geometry: rect(0, 250_000),
          lowerBound: true,
        },
        {
          shapeRef: "lower-folder",
          rowRef: "row-lower-folder",
          accessibleLabel: "Folder, at least 20 bytes",
          geometry: rect(250_000, 750_000),
          lowerBound: true,
          container: true,
        },
        {
          shapeRef: "exact",
          rowRef: "row-exact",
          accessibleLabel: "Exactly 10 bytes",
          geometry: rect(750_000, 1_000_000),
        },
      ]}
    />,
  );
  const edges = Array.from(
    container.querySelectorAll(".du-geometry-lower-bound"),
    (edge) => edge.getAttribute("d"),
  );
  // A 2 px band just inside the drawn edge: a leaf keeps a 1 px gap on its right
  // and bottom, a folder is drawn 1 px inside and its children 2 px inside.
  expect(edges).toEqual(["M48,0 V98 H0", "M148,1 V98 H51"]);
});

function labelled(width: number, name: string, value?: string) {
  const base = props();
  const { container } = render(
    <GeometryRenderer
      {...base}
      width={width}
      height={200}
      relations={[]}
      shapes={[
        {
          shapeRef: "label-shape",
          rowRef: "label-row",
          accessibleLabel: `${name} ${value ?? ""}`,
          displayLabel: value === undefined ? name : `${name} · ${value}`,
          displayName: name,
          ...(value === undefined ? {} : { displayValue: value }),
          geometry: {
            primitive: "rect",
            points: [
              { x: 0, y: 0 },
              { x: 500_000, y: 1_000_000 },
            ],
          },
        },
      ]}
    />,
  );
  return container.querySelector(".du-geometry-label")?.textContent;
}

test.each([
  // Half of 600 px leaves room for 19 characters.
  [600, "Music", "5.7 MiB", "Music · 5.7 MiB"],
  // Half of 300 px leaves room for 9: the value goes whole, then the name is cut.
  [300, "Music", "5.7 MiB", "Music"],
  // The name is cut in the middle, so its end (an extension) stays.
  [300, "LongFolderName", "5.7 MiB", "Long…Name"],
  [216, "notes.bin", "16 MiB", "no…bin"],
  [300, "Ab", "1 MiB", "Ab"],
  // Without a value the name is the label.
  [300, "Notes", undefined, "Notes"],
])(
  "a %s px chart labels %s with %s as %s, never cutting the value",
  (width, name, value, expected) => {
    expect(labelled(width, name, value)).toBe(expected);
  },
);
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
  ).toBe("99");
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
  const a = screen.getByRole("option", { name: "Item A, 10 bytes" });
  a.focus();
  // Item B lies outside this viewport, so End reaches the last drawn shape.
  fireEvent.keyDown(a, { key: "End" });
  expect(document.activeElement).toBe(
    screen.getByRole("option", { name: "Area" }),
  );
  fireEvent.click(a);
  expect(
    screen.getByRole("option", { name: "Item A, 10 bytes", selected: true }),
  ).toBeTruthy();
  expect(screen.getByRole("table", { name: "Same model rows" })).toBeTruthy();
});

test("graph pointer and keyboard alternative select the same source row", () => {
  const input = props();
  const onShapeFocus = vi.fn();
  const { container } = render(
    <GeometryRenderer {...input} onShapeFocus={onShapeFocus} />,
  );
  const first = screen.getByRole("option", { name: "Item A, 10 bytes" });
  const second = screen.getByRole("option", { name: "Item B, unknown" });
  fireEvent.click(first);
  expect(document.activeElement).toBe(first);
  expect(onShapeFocus).toHaveBeenLastCalledWith("row-a", "one", first);
  expect(input.onSelect).toHaveBeenLastCalledWith("row-a", "one");
  act(() => second.focus());
  expect(document.activeElement).toBe(second);
  expect(onShapeFocus).toHaveBeenLastCalledWith("row-b", "two", second);
  fireEvent.click(second);
  expect(input.onSelect).toHaveBeenLastCalledWith("row-b", "two");
  expect(container.querySelector(".du-geometry-label")?.textContent).toContain(
    "…",
  );
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
  expect(screen.queryAllByRole("option")).toHaveLength(0);
  expect(screen.getByText("Original row A")).toBeTruthy();
});

test("duplicate row references are not offered as the same item; the shape at the same place takes the focus", () => {
  const input = props();
  input.shapes = [
    input.shapes[0]!,
    { ...input.shapes[0]!, shapeRef: "duplicate" },
  ];
  input.relations = [];
  const availability = vi.fn();
  const moved = vi.fn();
  const { rerender } = render(
    <GeometryRenderer {...input} onFocusAvailability={availability} />,
  );
  expect(availability).toHaveBeenLastCalledWith(false);
  act(() => screen.getAllByRole("option")[0]!.focus());
  rerender(
    <GeometryRenderer
      {...input}
      modelRef="next"
      fallback={{ ...input.fallback, modelRef: "next" }}
      focusedRowRef="row-a"
      onFocusMoved={moved}
    />,
  );
  expect(document.activeElement).toBe(screen.getAllByRole("option")[0]);
  expect(moved).toHaveBeenCalledTimes(1);
});

test("subpixel geometry is not offered as an accessible focus target", () => {
  const input = props();
  input.shapes = [
    {
      ...input.shapes[0]!,
      geometry: {
        primitive: "rect",
        points: [
          { x: 0, y: 0 },
          { x: 1, y: 1 },
        ],
      },
    },
  ];
  input.relations = [];
  const availability = vi.fn();
  render(<GeometryRenderer {...input} onFocusAvailability={availability} />);
  expect(screen.queryAllByRole("option")).toHaveLength(0);
  expect(availability).toHaveBeenLastCalledWith(false);
});

test.each(["fallback", "replacement", "unmount"] as const)(
  "does not steal external focus on %s",
  (scenario) => {
    const input = props();
    const missing = vi.fn();
    const result = render(
      <>
        <button>Outside</button>
        <GeometryRenderer {...input} onFocusTargetMissing={missing} />
      </>,
    );
    screen.getByRole("option", { name: "Item A, 10 bytes" }).focus();
    const outside = screen.getByRole("button", { name: "Outside" });
    outside.focus();
    if (scenario === "unmount") {
      result.rerender(<button>Outside</button>);
    } else {
      const next = {
        ...input,
        modelRef: "next",
        fallback: { ...input.fallback, modelRef: "next" },
      };
      if (scenario === "fallback")
        next.viewport = { left: 0, top: 0, right: 0, bottom: 0 };
      result.rerender(
        <>
          <button>Outside</button>
          <GeometryRenderer
            {...next}
            focusedRowRef="row-a"
            onFocusTargetMissing={missing}
          />
        </>,
      );
    }
    expect(document.activeElement).toBe(outside);
    expect(missing).not.toHaveBeenCalled();
    if (scenario === "fallback")
      expect(screen.getByText("Original row A")).toBeTruthy();
  },
);
