import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import {
  adaptVizModel,
  selectionKey,
} from "../src/features/visualization/model-adapter";
import type { ModelRowsPage } from "../src/features/visualization/model-adapter";
import { shapeLabels } from "../src/features/visualization/model-text";
import {
  ModelRows,
  VisualizationModelPresenter,
} from "../src/features/visualization/VisualizationModelPresenter";

afterEach(cleanup);
const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
};
const adapt = ({ frame, page }: ReturnType<typeof modelFixture>) =>
  adaptVizModel(frame, page, (shape, row) =>
    shapeLabels(frame.model, shape, row, fixtureText),
  );

test("adapts original geometry and source values without recalculation or mutation", () => {
  const input = modelFixture();
  const before = JSON.stringify(input);
  const first = input.frame.model.shapes[0]!;
  input.frame.model.relationships.push({
    kind: "attribution",
    fromShapeRef: first.shapeRef!,
    toShapeRef: input.frame.model.shapes[1]!.shapeRef!,
  });
  const adapted = adapt(input);
  expect(adapted.shapes[0]!.geometry).toBe(first.geometry);
  expect(adapted.relations[0]!.fromShapeRef).toBe(adapted.shapes[0]!.shapeRef);
  expect(adapted.shapes[0]!.accessibleLabel).toContain("51539607552");
  input.frame.model.relationships.pop();
  expect(JSON.stringify(input)).toBe(before);
});

test.each([
  (page: ModelRowsPage) => {
    page.request.modelRef = "other-model";
  },
  (page: ModelRowsPage) => {
    page.request.resultRevision = "old";
  },
  (page: ModelRowsPage) => {
    page.response.resultRevision = "old";
  },
  (page: ModelRowsPage) => {
    page.response.completeness = "complete";
  },
  (page: ModelRowsPage) => {
    page.response.observedAt = "2026-11-11T00:00:00Z";
  },
  (page: ModelRowsPage) => {
    page.response.rows[0]!.itemRef = { resultVersion: "old", itemNo: 1 };
  },
])(
  "rejects a mismatched row page instead of attaching its names to this graph",
  (mutate) => {
    const input = modelFixture();
    mutate(input.page);
    const adapted = adapt(input);
    expect(adapted.rowsMatch).toBe(false);
    expect(adapted.rows).toEqual([]);
    expect(adapted.shapes[0]!.displayLabel).toContain(
      "Item name not yet retrieved",
    );
    expect(adapted.shapes[0]!.geometry).toBe(
      input.frame.model.shapes[0]!.geometry,
    );
  },
);

test("group selection stays model scoped and never selects its constituent files", () => {
  const input = modelFixture();
  const shape = input.frame.model.shapes[0]!;
  shape.itemRef = null;
  shape.groupRef = "small-items";
  delete shape.shapeRef;
  const adapted = adapt(input);
  const selection = adapted.targetsByShape.get(adapted.shapes[0]!.shapeRef)!;
  expect(selection).toEqual({
    kind: "group",
    groupRef: "small-items",
    resultRevision: input.frame.resultRevision,
    modelRef: input.frame.model.modelRef,
  });
  expect(selectionKey(selection, input.frame)).toBe(adapted.shapes[0]!.rowRef);
  input.frame.model.modelRef = "new-model";
  expect(selectionKey(selection, input.frame)).toBeNull();
});

test("identical names do not merge item selection and another revision is never highlighted", () => {
  const input = modelFixture();
  input.page.response.rows[1]!.displayName =
    input.page.response.rows[0]!.displayName;
  const adapted = adapt(input);
  const a = adapted.shapes[0]!.rowRef;
  const b = adapted.shapes[1]!.rowRef;
  expect(a).not.toBe(b);
  const selected = adapted.targetsByShape.get(adapted.shapes[0]!.shapeRef)!;
  input.frame.resultRevision = "new-result";
  expect(selectionKey(selected, input.frame)).toBeNull();
});

test("a foreign item in the model blocks rendering the whole mixed graph", () => {
  const input = modelFixture();
  input.frame.model.shapes[1]!.itemRef = { resultVersion: "old", itemNo: 2 };
  const { container } = render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
    />,
  );
  expect(container.querySelector("svg.du-geometry")).toBeNull();
  expect(screen.getByText(/inconsistent item references/)).toBeDefined();
});

test("pointer selection returns the generated identity and does not open details or navigate", () => {
  const input = modelFixture();
  const select = vi.fn();
  const { container } = render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={select}
      text={fixtureText}
    />,
  );
  fireEvent.click(container.querySelector(".du-geometry-shape rect")!);
  expect(select).toHaveBeenCalledWith(
    {
      kind: "item",
      itemRef: input.frame.model.shapes[0]!.itemRef,
      matchKey: input.frame.model.shapes[0]!.matchKey,
      modelRef: input.frame.model.modelRef,
      resultRevision: input.frame.resultRevision,
    },
    JSON.stringify(["shape", "fixture-shape-0"]),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("unsupported geometry shows same-model rows without drawing a partial graph", () => {
  const input = modelFixture();
  input.frame.model.shapes[1]!.geometry.primitive = "future-primitive";
  const { container } = render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
    />,
  );
  expect(container.querySelector("svg.du-geometry")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Rows from this graph" }));
  expect(
    screen.getByRole("table", { name: "Loaded graph rows" }).textContent,
  ).toContain("영상");
  expect(screen.getByText(/cannot be drawn/)).toBeDefined();
});

test("totals keep huge values, signed quantities, lower bounds and unknown source sizes separate", () => {
  const input = modelFixture();
  input.frame.model.totals.scope.quantity = {
    state: "lower_bound",
    value: "18446744073709551615",
    unit: "bytes",
  };
  input.frame.model.totals.scope.sourceSize = {
    state: "lower_bound",
    bytes: "18446744073709551615",
    basis: "logical",
    evidence: "measured",
  };
  input.frame.model.totals.drawn.quantity = {
    state: "exact",
    value: "-12.34567890123456789012",
    unit: "bytes",
  };
  input.frame.model.totals.grouped.quantity = {
    state: "unknown",
    unit: "bytes",
    reasonKey: "missing",
  };
  input.frame.model.totals.grouped.sourceSize = null;
  input.frame.model.totals.filteredOut.sourceSize = {
    state: "unknown",
    basis: "logical",
    evidence: "unknown",
  };
  render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
    />,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Measurement basis and totals" }),
  );
  expect(screen.getByText("At least 18446744073709551615 bytes")).toBeDefined();
  expect(screen.getByText("-12.34567890123456789012 bytes")).toBeDefined();
  expect(screen.getByText("No source size supplied")).toBeDefined();
  expect(
    screen.getByText("At least 18446744073709551615 bytes · Measured"),
  ).toBeDefined();
  expect(screen.getByText("Unavailable · Evidence unavailable")).toBeDefined();
});

test("a valid empty model shows its state without inventing zero-area shapes", () => {
  const input = modelFixture();
  input.frame.model.shapes = [];
  input.page.response.rows = [];
  input.page.response.totalCount = 0;
  const { container } = render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
    />,
  );
  expect(container.querySelector("svg.du-geometry")).toBeNull();
  expect(screen.getByText(/No drawable items/)).toBeDefined();
});

test("rows only expose Actions when this page has a group and an open handler", () => {
  const input = modelFixture();
  const groupRow = input.page.response.rows[0]!;
  groupRow.itemRef = null;
  groupRow.groupRef = "fixture-group";
  const { rerender } = render(
    <ModelRows
      page={input.page}
      model={input.frame.model}
      text={fixtureText}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Rows from this graph" }));
  let table = screen.getByRole("table", { name: "Loaded graph rows" });
  expect(
    within(table).queryByRole("columnheader", { name: "Actions" }),
  ).toBeNull();
  let bodyRows = within(table).getAllByRole("row").slice(1);
  expect(bodyRows).toHaveLength(4);
  expect(bodyRows.every((row) => row.children.length === 6)).toBe(true);

  rerender(
    <ModelRows
      page={input.page}
      model={input.frame.model}
      text={fixtureText}
      onOpenGroup={vi.fn()}
    />,
  );
  table = screen.getByRole("table", { name: "Loaded graph rows" });
  expect(
    within(table).getByRole("columnheader", { name: "Actions" }),
  ).toBeDefined();
  bodyRows = within(table).getAllByRole("row").slice(1);
  expect(bodyRows).toHaveLength(4);
  expect(bodyRows.every((row) => row.children.length === 7)).toBe(true);
  expect(
    within(bodyRows[0]!).getByRole("button", {
      name: "View members of 영상",
    }),
  ).toBeDefined();
  expect(within(bodyRows[1]!).queryByRole("button")).toBeNull();
});

test("rows omit Actions when the displayed page has no valid group row", () => {
  const input = modelFixture();
  // A group reference alone is not actionable when the row also identifies an item.
  input.page.response.rows[0]!.groupRef = "not-an-openable-group";
  render(
    <ModelRows
      page={input.page}
      model={input.frame.model}
      text={fixtureText}
      onOpenGroup={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Rows from this graph" }));
  const table = screen.getByRole("table", { name: "Loaded graph rows" });
  expect(
    within(table).queryByRole("columnheader", { name: "Actions" }),
  ).toBeNull();
  expect(
    within(table)
      .getAllByRole("row")
      .slice(1)
      .every((row) => row.children.length === 6),
  ).toBe(true);
});

test("Actions follows normal item and group page transitions with a handler", () => {
  const input = modelFixture();
  const onOpenGroup = vi.fn();
  const pageA = input.page;
  const groupRow = {
    ...pageA.response.rows[0]!,
    itemRef: null,
    groupRef: "fixture-group",
  };
  const pageB: ModelRowsPage = {
    request: { ...pageA.request, start: pageA.request.count },
    response: {
      ...pageA.response,
      rows: [groupRow],
      totalCount: pageA.response.totalCount + 1,
    },
  };
  const { rerender } = render(
    <ModelRows
      page={pageA}
      model={input.frame.model}
      text={fixtureText}
      onOpenGroup={onOpenGroup}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Rows from this graph" }));

  let table = screen.getByRole("table", { name: "Loaded graph rows" });
  expect(
    within(table).queryByRole("columnheader", { name: "Actions" }),
  ).toBeNull();
  let bodyRows = within(table).getAllByRole("row").slice(1);
  expect(bodyRows).toHaveLength(4);
  expect(bodyRows.every((row) => row.children.length === 6)).toBe(true);
  expect(
    within(table).queryByRole("button", { name: /View members of/ }),
  ).toBeNull();

  rerender(
    <ModelRows
      page={pageB}
      model={input.frame.model}
      text={fixtureText}
      onOpenGroup={onOpenGroup}
    />,
  );
  table = screen.getByRole("table", { name: "Loaded graph rows" });
  expect(
    within(table).getByRole("columnheader", { name: "Actions" }),
  ).toBeDefined();
  bodyRows = within(table).getAllByRole("row").slice(1);
  expect(bodyRows).toHaveLength(1);
  expect(bodyRows[0]!.children.length).toBe(7);
  expect(
    within(bodyRows[0]!).getByRole("button", {
      name: "View members of 영상",
    }),
  ).toBeDefined();

  rerender(
    <ModelRows
      page={pageA}
      model={input.frame.model}
      text={fixtureText}
      onOpenGroup={onOpenGroup}
    />,
  );
  table = screen.getByRole("table", { name: "Loaded graph rows" });
  expect(
    within(table).queryByRole("columnheader", { name: "Actions" }),
  ).toBeNull();
  bodyRows = within(table).getAllByRole("row").slice(1);
  expect(bodyRows).toHaveLength(4);
  expect(bodyRows.every((row) => row.children.length === 6)).toBe(true);
  expect(
    within(table).queryByRole("button", { name: /View members of/ }),
  ).toBeNull();
});

test("page navigation retains the exact model and result revision", () => {
  const input = modelFixture();
  input.page.response.totalCount = 24;
  const onPage = vi.fn();
  render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
      onPage={onPage}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Rows from this graph" }));
  fireEvent.click(screen.getByRole("button", { name: "Next rows" }));
  expect(onPage).toHaveBeenCalledWith({ ...input.page.request, start: 4 });
});

test("a loaded page does not imply that names of shapes outside the page are loading", () => {
  const input = modelFixture();
  input.page.response.rows = input.page.response.rows.slice(0, 2);
  input.page.request.count = 2;
  const adapted = adapt(input);
  expect(adapted.rowsMatch).toBe(true);
  expect(adapted.shapes[2]!.displayLabel).toBe(
    "Item name not yet retrieved · 17179869184 bytes · measured",
  );
  expect(adapted.shapes[2]!.geometry).toBe(
    input.frame.model.shapes[2]!.geometry,
  );
  render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
    />,
  );
  expect(screen.queryByRole("progressbar")).toBeNull();
  expect(
    screen.getAllByRole("button", { name: /Item name not yet retrieved/ }),
  ).toHaveLength(2);
});

test("row progress requires an explicit in-flight request for this model and revision", () => {
  const input = modelFixture();
  const props = {
    frame: input.frame,
    view,
    selected: null,
    onSelect: vi.fn(),
    text: fixtureText,
  };
  const { rerender } = render(<VisualizationModelPresenter {...props} />);
  expect(screen.queryByRole("progressbar")).toBeNull();
  rerender(
    <VisualizationModelPresenter
      {...props}
      pendingRowsRequest={input.page.request}
    />,
  );
  expect(
    screen.getByRole("progressbar", { name: "Loading rows from this graph" }),
  ).toBeDefined();
  rerender(
    <VisualizationModelPresenter
      {...props}
      pendingRowsRequest={{ ...input.page.request, resultRevision: "old" }}
    />,
  );
  expect(screen.queryByRole("progressbar")).toBeNull();
  rerender(
    <VisualizationModelPresenter
      {...props}
      pendingRowsRequest={{ ...input.page.request, modelRef: "old-model" }}
    />,
  );
  expect(screen.queryByRole("progressbar")).toBeNull();
});

test("repeated item shapes select their own evidence by pointer and keyboard while sharing the highlight", () => {
  const input = modelFixture();
  const first = input.frame.model.shapes[0]!;
  const second = input.frame.model.shapes[1]!;
  second.itemRef = first.itemRef;
  second.size = first.size;
  first.matchKey = { state: "unknown", reasonKey: "first-shape-evidence" };
  second.matchKey = { state: "unknown", reasonKey: "second-shape-evidence" };
  const select = vi.fn();
  const props = {
    ...input,
    view,
    selected: null,
    onSelect: select,
    text: fixtureText,
  };
  const { container, rerender } = render(
    <VisualizationModelPresenter {...props} />,
  );
  const shapes = container.querySelectorAll(".du-geometry-shape rect");
  fireEvent.click(shapes[0]!);
  expect(select).toHaveBeenLastCalledWith(
    expect.objectContaining({
      itemRef: first.itemRef,
      matchKey: first.matchKey,
    }),
    JSON.stringify(["shape", "fixture-shape-0"]),
  );
  fireEvent.click(shapes[1]!);
  expect(select).toHaveBeenLastCalledWith(
    expect.objectContaining({
      itemRef: second.itemRef,
      matchKey: second.matchKey,
    }),
    JSON.stringify(["shape", "fixture-shape-1"]),
  );
  // React Aria's keyboard press path must carry the same per-shape reference.
  const buttons = screen.getAllByRole("button", { name: /^영상 ·/ });
  buttons[0]!.focus();
  fireEvent.keyDown(buttons[0]!, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(buttons[0]!, { key: "Enter", code: "Enter" });
  expect(select).toHaveBeenLastCalledWith(
    expect.objectContaining({ matchKey: first.matchKey }),
    JSON.stringify(["shape", "fixture-shape-0"]),
  );
  rerender(
    <VisualizationModelPresenter
      {...props}
      selected={{
        kind: "item",
        itemRef: first.itemRef!,
        matchKey: first.matchKey,
        resultRevision: input.frame.resultRevision,
        modelRef: input.frame.model.modelRef,
      }}
    />,
  );
  expect(
    container.querySelectorAll('.du-geometry-shape[data-selected="true"]'),
  ).toHaveLength(2);
});
