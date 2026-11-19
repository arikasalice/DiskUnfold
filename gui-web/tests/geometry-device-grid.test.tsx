import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { GeometryRenderer } from "../src/components/charts/geometry-renderer";
import type { GeometryRendererProps } from "../src/components/charts/geometry-renderer";
import { measureGrid, snap, whole } from "../src/components/charts/device-grid";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const box = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height }) as DOMRect;

test("F06 FR12.HD1.gui-web.DD1: the grid puts the chart origin on a device pixel at any ratio and app scale", () => {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  // 200 user units drawn 250 CSS px wide: an app scale of 125%.
  vi.spyOn(svg, "getBoundingClientRect").mockReturnValue(
    box(30.4, 333.5, 250, 125),
  );
  const zoomed = measureGrid(svg, 200, 1);
  expect(zoomed.scale).toBeCloseTo(1.25);
  // The shift moves the origin to the next device pixel: 30.4 -> 30, 333.5 -> 334.
  expect(30.4 + zoomed.dx * zoomed.scale).toBeCloseTo(30);
  expect(333.5 + zoomed.dy * zoomed.scale).toBeCloseTo(334);
  const ratio = measureGrid(svg, 250, 2);
  expect(ratio.scale).toBeCloseTo(2);
  expect((30.4 * 2 + ratio.dx * ratio.scale) % 1).toBeCloseTo(0);
  // Positions and widths land on whole device pixels.
  expect(snap(10.3, zoomed) * zoomed.scale).toBeCloseTo(13);
  expect(whole(1, zoomed) * zoomed.scale).toBe(1);
  expect(whole(2, zoomed) * zoomed.scale).toBe(3);
  expect(whole(3, ratio) * ratio.scale).toBe(6);
});

test("F06 FR12.HD1.gui-web.DD1: at an app scale of 125% every rectangle edge and band falls on a device pixel", () => {
  vi.spyOn(SVGElement.prototype, "getBoundingClientRect").mockReturnValue(
    box(30.4, 333.5, 250, 125),
  );
  const shapes: GeometryRendererProps["shapes"] = [
    {
      shapeRef: "lower",
      rowRef: "row-lower",
      accessibleLabel: "At least 10 bytes",
      lowerBound: true,
      geometry: {
        primitive: "rect",
        points: [
          { x: 0, y: 0 },
          { x: 333_333, y: 1_000_000 },
        ],
      },
    },
    {
      shapeRef: "folder",
      rowRef: "row-folder",
      accessibleLabel: "Folder",
      container: true,
      geometry: {
        primitive: "rect",
        points: [
          { x: 333_333, y: 0 },
          { x: 1_000_000, y: 1_000_000 },
        ],
      },
    },
  ];
  const { container } = render(
    <GeometryRenderer
      modelRef="model"
      resultRevision="revision"
      label="Chart"
      selectedText="selected"
      fallbackNotice="Unavailable"
      missingFallbackNotice="Missing"
      width={200}
      height={100}
      viewport={{ left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 }}
      shapes={shapes}
      relations={[]}
      selectedRowRef="row-folder"
      onSelect={() => {}}
      fallback={{
        modelRef: "model",
        resultRevision: "revision",
        content: null,
      }}
    />,
  );
  const scale = 1.25;
  const onPixel = (value: string | null) =>
    expect(Math.abs((Number(value) * scale) % 1)).toBeLessThan(1e-9);
  const root = container.querySelector("svg > g");
  expect(root?.getAttribute("transform")).toMatch(/^translate\(/);
  for (const rect of container.querySelectorAll(
    ".du-geometry-shape > rect:not(.du-geometry-selection)",
  )) {
    const x = Number(rect.getAttribute("x"));
    const y = Number(rect.getAttribute("y"));
    for (const edge of [
      x,
      y,
      x + Number(rect.getAttribute("width")),
      y + Number(rect.getAttribute("height")),
    ])
      onPixel(String(edge));
  }
  // The dash and the selection outline are bands of whole device pixels.
  const dash = container.querySelector(".du-geometry-lower-bound");
  expect(Number(dash?.getAttribute("stroke-width")) * scale).toBeCloseTo(3);
  const selection = container.querySelector(".du-geometry-selection");
  expect(Number(selection?.getAttribute("stroke-width")) * scale).toBeCloseTo(
    4,
  );
});
