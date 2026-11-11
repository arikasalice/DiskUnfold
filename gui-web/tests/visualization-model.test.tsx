import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import {
  adaptVizModel,
  selectionKey,
} from "../src/features/visualization/model-adapter";
import type { ModelRowsPage } from "../src/features/visualization/model-adapter";
import { shapeLabels } from "../src/features/visualization/model-text";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";

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
    expect(adapted.shapes[0]!.displayLabel).toContain("Item details loading");
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
  const selection = adapted.targets.get(adapted.shapes[0]!.rowRef)!;
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
  const selected = adapted.targets.get(a)!;
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
  expect(select).toHaveBeenCalledWith({
    kind: "item",
    itemRef: input.frame.model.shapes[0]!.itemRef,
    matchKey: input.frame.model.shapes[0]!.matchKey,
    modelRef: input.frame.model.modelRef,
    resultRevision: input.frame.resultRevision,
  });
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
