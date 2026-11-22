import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { ControlledVisualization } from "../src/features/visualization/ControlledVisualization";
import { VisualizationController } from "../src/features/visualization/controller";
import { GroupController } from "../src/features/visualization/group-controller";
import { TypeList } from "../src/features/visualization/TypeList";
import {
  intervalLabel,
  intervalListing,
  intervalRow,
} from "../src/features/visualization/interval-list";
import type {
  QueryCondition,
  VizRenderRequest,
  VizRows,
  VizRowsRequest,
} from "../src/ipc";
import {
  createProductionVizText,
  shapeLabels,
  visualizationMessage,
} from "../src/viz";
import type { VizText } from "../src/viz";

afterEach(cleanup);

const ko: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);
const say = (key: Parameters<typeof visualizationMessage>[1]) =>
  visualizationMessage("ko", key);
const MiB = 1024 * 1024;
const size = (op: "at_least" | "less_than", bytes: number): QueryCondition => ({
  kind: "predicate",
  predicate: { kind: "size", basis: "logical", op, bytes: String(bytes) },
});
const interval = (lower: number, upper?: number): QueryCondition => ({
  kind: "all",
  children: [
    { kind: "predicate", predicate: { kind: "type", value: "file" } },
    size("at_least", lower),
    ...(upper === undefined ? [] : [size("less_than", upper)]),
  ],
});

/** A histogram whose first row is the interval [1 MiB, 10 MiB) of six files. */
function histogram(condition: QueryCondition | undefined, count = "6") {
  const input = modelFixture();
  const model = input.frame.model;
  model.representation = { ...model.representation, format: "histogram" };
  const shape = model.shapes[0]!;
  shape.itemRef = null;
  shape.groupRef = "bin-3";
  shape.memberCount = { unit: "items", state: "exact", value: count };
  const row = input.page.response.rows[0]!;
  input.page.response.rows[0] = {
    ...row,
    itemRef: null,
    groupRef: "bin-3",
    displayName: "1 MiB–10 MiB",
    memberCount: shape.memberCount,
    ...(condition ? { condition } : {}),
  };
  return { input, shape, row: input.page.response.rows[0]! };
}

test("F06 README 52: an interval is named by its exact bounds, the last one open above", () => {
  expect(intervalLabel(interval(MiB, 10 * MiB), ko)).toBe(
    "크기 1 MiB 이상 10 MiB 미만",
  );
  expect(intervalLabel(interval(1024 * 1024 * MiB), ko)).toBe(
    "크기 1 TiB 이상",
  );
});

test("F06 README 52: an interval with a list says so in its name and card; without one only its card says why", () => {
  const listed = histogram(interval(MiB, 10 * MiB));
  const opens = shapeLabels(
    listed.input.frame.model,
    listed.shape,
    listed.row,
    ko,
  );
  expect(opens.accessibleLabel.endsWith(`, ${say("intervalShowInList")}`)).toBe(
    true,
  );
  expect(JSON.stringify(opens.card)).toContain(say("intervalCardOpens"));

  const unlisted = histogram(undefined);
  const none = shapeLabels(
    unlisted.input.frame.model,
    unlisted.shape,
    unlisted.row,
    ko,
  );
  expect(none.accessibleLabel).not.toContain(say("intervalShowInList"));
  expect(JSON.stringify(none.card)).toContain(say("intervalCardNoList"));

  const empty = histogram(interval(MiB, 10 * MiB), "0");
  const nothing = shapeLabels(
    empty.input.frame.model,
    empty.shape,
    empty.row,
    ko,
  );
  expect(nothing.accessibleLabel).not.toContain(say("intervalShowInList"));
  expect(JSON.stringify(nothing.card)).not.toContain(say("intervalCardOpens"));
});

test("F06 README 52: the listing carries the bar's count and size, and an empty interval opens nothing", () => {
  const { input, row } = histogram(interval(MiB, 10 * MiB));
  expect(intervalRow(input.page, "bin-3")).toBe(row);
  const listing = intervalListing(row, input.frame, ko, null)!;
  expect(listing.label).toBe("크기 1 MiB 이상 10 MiB 미만");
  expect(listing.summary.count).toEqual(row.memberCount);
  expect(listing.summary.sizeBasis).toBe("logical");
  expect(listing.condition).toEqual(row.condition);
  const empty = histogram(interval(MiB, 10 * MiB), "0");
  expect(intervalRow(empty.input.page, "bin-3")).toBeNull();
  expect(intervalRow(histogram(undefined).input.page, "bin-3")).toBeNull();
});

function request(): VizRenderRequest {
  const revision = modelFixture().frame.resultRevision;
  return {
    viewChannel: "interval-list-test",
    requestSeq: 1,
    resultRevision: revision,
    format: "histogram",
    aggregateScopeRef: { resultVersion: revision, itemNo: 0 },
    navigationScopeRef: { resultVersion: revision, itemNo: 0 },
    measure: "count",
    sizeBasis: "logical",
    classificationCriterion: "type",
    colorCriterion: "none",
    denominatorBasis: "scope",
    minAreaPx2: 1,
    depthLimit: 2,
    viewport: { width: 960, height: 330, scale: 1 },
    visibleWindow: { left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 },
  };
}

async function table(condition: QueryCondition | undefined) {
  const { input } = histogram(condition);
  input.page.request.count = 100;
  const rows = vi.fn(async (request: VizRowsRequest): Promise<VizRows> => ({
    ...input.page.response,
    resultRevision: request.resultRevision,
  }));
  const visualization = new VisualizationController({
    render: vi.fn().mockResolvedValue(input.frame),
    rows,
    cancel: vi.fn(),
    canDisplay: () => false,
  });
  const members = vi.fn().mockResolvedValue({
    rows: [],
    totalCount: 0,
    groupTotal: input.page.response.rows[0]!.size,
    sizeRange: { kind: "unknown" },
    resultRevision: input.frame.resultRevision,
    observedAt: input.frame.observedAt,
    completeness: input.frame.completeness,
  });
  const onOpenList = vi.fn();
  render(
    <ControlledVisualization
      controller={visualization}
      groupController={new GroupController({ rows: members })}
      text={fixtureText}
      autoLoadRows
      onOpenList={onOpenList}
    />,
  );
  await act(async () => visualization.render(request()));
  fireEvent.click(screen.getByRole("button", { name: /Items by size range/ }));
  const open = screen.getByRole("button", { name: /^View items in 1 MiB/ });
  return { open, onOpenList, members };
}

test("F06 README 52: the table's button opens the interval's list, and the row itself opens nothing", async () => {
  const { open, onOpenList, members } = await table(interval(MiB, 10 * MiB));
  fireEvent.click(screen.getByText("1 MiB–10 MiB"));
  expect(onOpenList).not.toHaveBeenCalled();
  fireEvent.click(open);
  expect(onOpenList).toHaveBeenCalledTimes(1);
  const listing = onOpenList.mock.calls[0]![0];
  expect(listing.condition).toEqual(interval(MiB, 10 * MiB));
  // The button takes the focus back when the list closes.
  expect(listing.origin).toBe(open);
  expect(members).not.toHaveBeenCalled();
});

test("F06 README 52: an interval without a list condition keeps opening its members", async () => {
  const { open, onOpenList, members } = await table(undefined);
  fireEvent.click(open);
  expect(onOpenList).not.toHaveBeenCalled();
  expect(members).toHaveBeenCalled();
});

test("F06 README 52: the interval's list says its bounds, count, size and basis on one line, with no type chip", () => {
  const { input, row } = histogram(interval(MiB, 10 * MiB));
  const listing = intervalListing(row, input.frame, ko, null)!;
  render(
    <TypeList
      source={{ open: () => new Promise(() => {}), next: vi.fn() }}
      resultVersion={input.frame.resultRevision}
      scope={input.frame.model.aggregateScopeRef}
      condition={listing.condition}
      type={listing.label}
      interval={listing.label}
      summary={listing.summary}
      text={ko}
      onBack={vi.fn()}
    />,
  );
  const line = screen.getByText(/^크기 1 MiB 이상 10 MiB 미만 · /);
  expect(line.textContent).toContain(ko.key("logical"));
  expect(screen.queryByRole("button", { name: /유형 조건 지우기/ })).toBeNull();
  expect(screen.getByRole("button", { name: say("listBack") })).toBeTruthy();
});
