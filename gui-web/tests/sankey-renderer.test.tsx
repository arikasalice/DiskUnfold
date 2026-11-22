import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { ModelRows } from "../src/features/visualization/VisualizationModelPresenter";
import {
  SankeyRenderer,
  fitLabel,
} from "../src/components/charts/sankey-renderer";
import { columnStep, keptLinks } from "../src/components/charts/sankey-links";
import type { GeometryRendererProps } from "../src/components/charts/geometry-renderer";
import { sankeyLinkWords, withParticle } from "../src/viz/sankey-text";

afterEach(cleanup);

// F06 README 12, 12.1 (#296): folders → types, with a volume column only for
// several scope roots, under the names of the columns.

const LIMIT = 1_000_000;
const rect = (left: number, top: number, right: number, bottom: number) => ({
  primitive: "rect",
  points: [
    { x: left, y: top },
    { x: right, y: bottom },
  ],
});

test("a label keeps its value whole: the name shortens, then the share goes, then the name", () => {
  const shape = node("t", "type", "그 밖의 유형", rect(0, 0, 1, 1), {
    displayValue: "48.0 MiB",
    displayShare: "21.1%",
  });
  expect(fitLabel(shape, "type", 207)).toBe("그 밖의 유형 · 48.0 MiB · 21.1%");
  const narrower = fitLabel(shape, "type", 150);
  expect(narrower).toMatch(/….* · 48\.0 MiB$/);
  expect(narrower).not.toContain("%");
  expect(fitLabel(shape, "type", 60)).toBe("48.0 MiB");
  expect(fitLabel(shape, "type", 40)).toBe("");
});
const band = (x1: number, y1: number, x2: number, y2: number, h: number) => ({
  primitive: "polygon",
  points: [
    { x: x1, y: y1 },
    { x: x2, y: y2 },
    { x: x2, y: y2 + h },
    { x: x1, y: y1 + h },
  ],
});
const node = (
  shapeRef: string,
  column: string,
  name: string,
  geometry: ReturnType<typeof rect>,
  extra: Partial<GeometryRendererProps["shapes"][number]> = {},
) => ({
  shapeRef,
  rowRef: `row-${shapeRef}`,
  accessibleLabel: name,
  displayName: name,
  displayValue: "1 GiB",
  displayShare: "10%",
  geometry,
  column,
  ...extra,
});

/** One scope root C:, or two roots C: (to Projects) and D: (to Media). */
function props(roots: 1 | 2 = 1): GeometryRendererProps {
  const scopes =
    roots === 1
      ? [
          node("c", "scope", "C:", rect(0, 0, 12_000, LIMIT), {
            tone: "folder",
          }),
        ]
      : [
          node("c", "scope", "C:", rect(0, 0, 12_000, 600_000), {
            tone: "folder",
          }),
          node("d", "scope", "D:", rect(0, 620_000, 12_000, LIMIT), {
            tone: "folder",
          }),
        ];
  return {
    modelRef: "model-1",
    resultRevision: "revision-1",
    label: "Space distribution",
    selectedText: "selected",
    fallbackNotice: "Graph unavailable",
    missingFallbackNotice: "Table unavailable",
    width: 1000,
    height: 400,
    viewport: { left: 0, top: 0, right: LIMIT, bottom: LIMIT },
    selectedRowRef: null,
    onSelect: vi.fn(),
    fallback: {
      modelRef: "model-1",
      resultRevision: "revision-1",
      content: null,
    },
    columnNames: { scope: "Volume", folder: "Folder", type: "Type" },
    shapes: [
      ...scopes,
      node("a", "folder", "Projects", rect(494_000, 0, 506_000, 600_000), {
        tone: "folder",
      }),
      node("b", "folder", "Media", rect(494_000, 620_000, 506_000, LIMIT), {
        tone: "folder",
      }),
      node("video", "type", "Video", rect(988_000, 0, LIMIT, 500_000), {
        tone: "5",
      }),
      node(
        "none",
        "type",
        "No extension",
        rect(988_000, 520_000, LIMIT, LIMIT),
        { tone: "neutral-2", pattern: "sparse-dots", patternAlways: true },
      ),
    ],
    relations: [
      {
        fromShapeRef: "c",
        toShapeRef: "a",
        geometry: band(12_000, 0, 494_000, 0, 600_000),
      },
      {
        fromShapeRef: roots === 1 ? "c" : "d",
        toShapeRef: "b",
        geometry: band(12_000, 620_000, 494_000, 620_000, 380_000),
      },
      {
        fromShapeRef: "a",
        toShapeRef: "video",
        geometry: band(506_000, 0, 988_000, 0, 500_000),
      },
      {
        fromShapeRef: "a",
        toShapeRef: "none",
        geometry: band(506_000, 500_000, 988_000, 520_000, 100_000),
      },
      {
        fromShapeRef: "b",
        toShapeRef: "none",
        geometry: band(506_000, 620_000, 988_000, 620_000, 380_000),
      },
    ],
  };
}

const option = (name: string) => screen.getByRole("option", { name });
const links = (container: HTMLElement) =>
  Array.from(container.querySelectorAll(".du-geometry-relation"));
const texts = (container: HTMLElement, selector: string) =>
  Array.from(container.querySelectorAll(selector)).map(
    (element) => element.textContent,
  );

test("a lone scope root is left out: two named columns, folders labelled with their value, types with value and share", () => {
  const { container } = render(<SankeyRenderer {...props()} />);
  expect(screen.queryByRole("option", { name: "C:" })).toBeNull();
  expect(links(container)).toHaveLength(3);
  expect(texts(container, ".du-geometry-sankey-heading")).toEqual([
    "Folder",
    "Type",
  ]);
  const labels = texts(container, ".du-geometry-sankey-label");
  expect(labels).toContain("Projects · 1 GiB");
  expect(labels).toContain("Video · 1 GiB · 10%");
});

test("several scope roots keep the volume column and its name", () => {
  const { container } = render(<SankeyRenderer {...props(2)} />);
  expect(option("C:")).toBeTruthy();
  expect(option("D:")).toBeTruthy();
  expect(links(container)).toHaveLength(5);
  expect(texts(container, ".du-geometry-sankey-heading")).toEqual([
    "Volume",
    "Folder",
    "Type",
  ]);
});

test("a band into a type takes the type's colour and pattern, a band into a folder the muted band", () => {
  const { container } = render(<SankeyRenderer {...props(2)} />);
  const [toA, , toVideo, toNone] = links(container);
  expect(toA!.getAttribute("data-band")).toBe("scope");
  expect(toA!.getAttribute("data-tone")).toBeNull();
  expect(toVideo!.getAttribute("data-band")).toBe("type");
  expect(toVideo!.getAttribute("data-tone")).toBe("5");
  // The neutral "No extension" band always carries its sparse dots.
  expect(toNone!.getAttribute("data-tone")).toBe("neutral-2");
  expect(toNone!.querySelector(".du-geometry-pattern")).not.toBeNull();
});

test("pointing at a node keeps only its links: the others fade, its own are marked kept", () => {
  const { container } = render(<SankeyRenderer {...props()} />);
  fireEvent.pointerEnter(option("Projects"));
  expect(
    links(container).map((link) => link.hasAttribute("data-faded")),
  ).toEqual([false, false, true]);
  expect(
    links(container).map((link) => link.hasAttribute("data-kept")),
  ).toEqual([true, true, false]);
  fireEvent.pointerLeave(option("Projects"));
  expect(
    links(container).some(
      (link) =>
        link.hasAttribute("data-faded") || link.hasAttribute("data-kept"),
    ),
  ).toBe(false);
});

test("left and right move between columns, up and down within one", () => {
  render(<SankeyRenderer {...props()} />);
  option("Projects").focus();
  const press = (key: string) =>
    fireEvent.keyDown(document.activeElement!, { key });
  press("ArrowLeft");
  expect(document.activeElement).toBe(option("Projects"));
  press("ArrowRight");
  expect(document.activeElement).toBe(option("Video"));
  press("ArrowDown");
  expect(document.activeElement).toBe(option("No extension"));
  press("ArrowLeft");
  expect(document.activeElement).toBe(option("Media"));
  press("ArrowUp");
  expect(document.activeElement).toBe(option("Projects"));
});

test("column steps stop at the edges and pick the nearest node of the next column", () => {
  const box = (top: number, bottom: number) => ({
    left: 0,
    right: 1,
    top,
    bottom,
  });
  const items = [
    { column: "scope", box: box(0, 100) },
    { column: "folder", box: box(0, 20) },
    { column: "folder", box: box(40, 100) },
  ];
  expect(columnStep(items, 0, "ArrowLeft")).toBeUndefined();
  expect(columnStep(items, 0, "ArrowRight")).toBe(2);
  expect(columnStep(items, 2, "ArrowDown")).toBeUndefined();
  expect(columnStep(items, 2, "ArrowUp")).toBe(1);
});

test("a pointed link keeps itself; nothing pointed keeps every link", () => {
  const relations = [
    { fromShapeRef: "a", toShapeRef: "b" },
    { fromShapeRef: "b", toShapeRef: "c" },
  ];
  expect(keptLinks(relations, null)).toBeNull();
  expect([...keptLinks(relations, { link: 1 })!]).toEqual([1]);
  expect([...keptLinks(relations, { shapeRef: "b" })!]).toEqual([0, 1]);
});

test("a link is named from, to and its amount, with the Korean particle after the type's name", () => {
  expect(withParticle("민서에서 영상(으)로, 80 GiB")).toBe(
    "민서에서 영상으로, 80 GiB",
  );
  expect(withParticle("민서에서 문서(으)로, 1 GiB")).toBe(
    "민서에서 문서로, 1 GiB",
  );
  expect(withParticle("민서에서 파일(으)로, 1 GiB")).toBe(
    "민서에서 파일로, 1 GiB",
  );
  expect(withParticle("A에서 Video(으)로, 1 GiB")).toBe(
    "A에서 Video(으)로, 1 GiB",
  );
  const words = sankeyLinkWords(
    { legend: [] },
    {
      links: [
        {
          toRowRef: "group:viz.sankey.type.type.video",
          quantity: { unit: "bytes", state: "exact", value: "80" },
        },
        {
          toRowRef: "group:viz.sankey.type.unattributed",
          quantity: { unit: "bytes", state: "exact", value: "2" },
        },
      ],
    },
    "민서",
    {
      message: () => "{from}에서 {to}(으)로, {value}",
      key: () => "나눌 수 없는 공간",
      quantity: (value) => `${"value" in value ? value.value : "?"} B`,
      className: (key) => (key === "type.video" ? "영상" : key),
    },
  );
  expect(words).toEqual([
    "민서에서 영상으로, 80 B",
    "민서에서 나눌 수 없는 공간으로, 2 B",
  ]);
});

test("the table lists where each Sankey node row sends its space", () => {
  const input = modelFixture();
  input.frame.model.legend = [
    {
      ...input.frame.model.legend[0]!,
      key: "type.video",
      labelKey: "ui.visualization.legend.type.video",
    },
  ];
  const [first, ...rest] = input.page.response.rows;
  input.page.response.rows = [
    {
      ...first!,
      links: [
        {
          toRowRef: "group:viz.sankey.type.type.video",
          quantity: { unit: "bytes", state: "exact", value: "1024" },
        },
      ],
    },
    ...rest,
  ];
  render(
    <ModelRows
      page={input.page}
      model={input.frame.model}
      text={fixtureText}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Items in this folder" }));
  const table = screen.getByRole("table", { name: "Items in this folder" });
  expect(
    within(table).getByRole("columnheader", { name: "Linked items" }),
  ).toBeTruthy();
  const cells = within(table).getAllByRole("row")[1]!.querySelectorAll("td");
  // The fixture text words the amount; the cell names the target and its amount.
  expect(cells[cells.length - 1]!.textContent).toMatch(/ 1024 byt/);
});
