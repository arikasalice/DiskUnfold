import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { ControlledVisualization } from "../src/features/visualization/ControlledVisualization";
import { VisualizationController } from "../src/features/visualization/controller";
import { GroupController } from "../src/features/visualization/group-controller";
import type {
  VizGroupRows,
  VizRenderRequest,
  VizRows,
  VizRowsRequest,
} from "../src/ipc";
import type {
  ModelResponse,
  ModelRowsPage,
} from "../src/features/visualization/model-adapter";

afterEach(cleanup);

const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 },
};
const sharedGroup = "shared-group-ref";

function frame(
  modelRef: string,
  resultRevision: string,
  requestSeq: number,
  completeness: ModelResponse["completeness"],
): ModelResponse {
  const value = modelFixture().frame;
  value.model.modelRef = modelRef;
  value.resultRevision = resultRevision;
  value.requestSeq = requestSeq;
  value.observedAt = `2026-11-12T10:00:0${requestSeq}Z`;
  value.completeness = completeness;
  value.model.aggregateScopeRef.resultVersion = resultRevision;
  value.model.navigationScopeRef.resultVersion = resultRevision;
  value.model.shapes = value.model.shapes.map((shape) => ({
    ...shape,
    ...(shape.itemRef
      ? { itemRef: { ...shape.itemRef, resultVersion: resultRevision } }
      : {}),
    ...(shape.rootBranchRef
      ? {
          rootBranchRef: {
            ...shape.rootBranchRef,
            resultVersion: resultRevision,
          },
        }
      : {}),
  }));
  value.model.shapes[0] = {
    ...value.model.shapes[0]!,
    itemRef: null,
    groupRef: sharedGroup,
  };
  return value;
}

function renderRequest(value: ModelResponse): VizRenderRequest {
  return {
    viewChannel: "newer-table-group-test",
    requestSeq: value.requestSeq,
    resultRevision: value.resultRevision,
    format: "treemap",
    aggregateScopeRef: value.model.aggregateScopeRef,
    navigationScopeRef: value.model.navigationScopeRef,
    measure: "size",
    sizeBasis: "logical",
    classificationCriterion: "type",
    colorCriterion: "type",
    denominatorBasis: "scope",
    minAreaPx2: 1,
    depthLimit: 2,
    ...view,
  };
}

function rowsPage(value: ModelResponse): ModelRowsPage {
  const page = modelFixture().page;
  const row = page.response.rows[0]!;
  page.request = {
    modelRef: value.model.modelRef,
    resultRevision: value.resultRevision,
    start: 0,
    count: 100,
  };
  page.response = {
    ...page.response,
    resultRevision: value.resultRevision,
    observedAt: value.observedAt,
    completeness: value.completeness,
    totalCount: 1,
    rows: [
      {
        ...row,
        itemRef: null,
        groupRef: sharedGroup,
        displayName: "Shared group",
      },
    ],
  };
  return page;
}

function groupPage(value: ModelResponse): VizGroupRows {
  const row = modelFixture().page.response.rows[0]!;
  return {
    rows: [
      {
        ...row,
        itemRef: { resultVersion: value.resultRevision, itemNo: 1 },
        groupRef: null,
        displayName: `Member from ${value.model.modelRef}`,
      },
    ],
    totalCount: 1,
    groupTotal: row.size,
    sizeRange: { kind: "known", min: row.size, max: row.size },
    resultRevision: value.resultRevision,
    observedAt: value.observedAt,
    completeness: value.completeness,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve: (value: T) => resolve(value) };
}

function pressEnter(target: HTMLElement) {
  target.focus();
  fireEvent.keyDown(target, { key: "Enter" });
  fireEvent.keyUp(target, { key: "Enter" });
}

test("newer table groups use their own frame and return to their own row", async () => {
  const graph = frame("old-graph-model", "old-result", 1, "partial");
  const newer = frame("new-table-model", "new-result", 2, "complete");
  const newest = frame("newest-table-model", "newest-result", 3, "complete");
  const pages = new Map(
    [graph, newer, newest].map((value) => [
      value.model.modelRef,
      rowsPage(value),
    ]),
  );
  const visualization = new VisualizationController({
    render: vi
      .fn()
      .mockResolvedValueOnce(graph)
      .mockResolvedValueOnce(newer)
      .mockResolvedValueOnce(newest),
    rows: vi.fn(async (request: VizRowsRequest): Promise<VizRows> => {
      const page = pages.get(request.modelRef)!.response;
      return { ...page, rows: page.rows };
    }),
    cancel: vi.fn(),
    canDisplay: (value) => value.model.modelRef === graph.model.modelRef,
  });
  const oldResponse = deferred<VizGroupRows>();
  const lateTableResponse = deferred<VizGroupRows>();
  let tableRequests = 0;
  const groupPort = vi.fn(
    (request: {
      modelRef: string;
      groupRef: string;
      start: number;
      count: number;
    }) => {
      if (request.modelRef === graph.model.modelRef) return oldResponse.promise;
      tableRequests += 1;
      if (tableRequests === 1) return Promise.resolve(groupPage(newer));
      return lateTableResponse.promise;
    },
  );
  const groups = new GroupController({ rows: groupPort });

  render(
    <ControlledVisualization
      controller={visualization}
      groupController={groups}
      text={fixtureText}
      autoLoadRows
    />,
  );
  await act(async () => visualization.render(renderRequest(graph)));
  expect(visualization.getSnapshot().error).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Rows from this graph" }));
  act(() =>
    visualization.select({
      kind: "group",
      groupRef: sharedGroup,
      modelRef: graph.model.modelRef,
      resultRevision: graph.resultRevision,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "View group members" }));
  await waitFor(() =>
    expect(groupPort).toHaveBeenLastCalledWith({
      modelRef: graph.model.modelRef,
      groupRef: sharedGroup,
      start: 0,
      count: 100,
    }),
  );

  await act(async () => visualization.render(renderRequest(newer)));
  fireEvent.click(screen.getByRole("button", { name: "Items in this result" }));
  const groupRows = screen.getAllByRole("button", {
    name: "View members of Shared group",
  });
  const newerTableRow = groupRows[groupRows.length - 1]!;
  newerTableRow.focus();
  fireEvent.keyDown(newerTableRow, { key: "Enter" });
  fireEvent.keyUp(newerTableRow, { key: "Enter" });
  await screen.findByText(`Member from ${newer.model.modelRef}`);
  expect(groups.getSnapshot()).toMatchObject({
    frame: {
      modelRef: newer.model.modelRef,
      resultRevision: newer.resultRevision,
      observedAt: newer.observedAt,
      completeness: newer.completeness,
    },
    groupRef: sharedGroup,
  });
  expect(groupPort).toHaveBeenLastCalledWith({
    modelRef: newer.model.modelRef,
    groupRef: sharedGroup,
    start: 0,
    count: 100,
  });

  await act(async () => oldResponse.resolve(groupPage(graph)));
  expect(groups.getSnapshot()).toMatchObject({
    frame: { modelRef: newer.model.modelRef },
    groupRef: sharedGroup,
    status: "ready",
    page: {
      response: {
        rows: [{ displayName: `Member from ${newer.model.modelRef}` }],
      },
    },
  });
  const back = screen.getByRole("button", { name: "Back to visualization" });
  back.focus();
  fireEvent.keyDown(back, { key: "Enter" });
  fireEvent.keyUp(back, { key: "Enter" });
  expect(document.activeElement).toBe(newerTableRow);

  const refreshedRows = screen.getAllByRole("button", {
    name: "View members of Shared group",
  });
  const refreshedTableRow = refreshedRows[refreshedRows.length - 1]!;
  refreshedTableRow.focus();
  fireEvent.keyDown(refreshedTableRow, { key: "Enter" });
  fireEvent.keyUp(refreshedTableRow, { key: "Enter" });
  await waitFor(() => expect(tableRequests).toBe(2));
  await act(async () => visualization.render(renderRequest(newest)));
  await waitFor(() =>
    expect(groups.getSnapshot()).toMatchObject({
      frame: { modelRef: newer.model.modelRef },
      groupRef: null,
      status: "closed",
      page: null,
    }),
  );
  await act(async () => lateTableResponse.resolve(groupPage(newer)));
  expect(groups.getSnapshot()).toMatchObject({
    frame: { modelRef: newer.model.modelRef },
    groupRef: null,
    status: "closed",
    page: null,
  });
});

test("graph and newer table rows with one group ref use and return to their own model", async () => {
  const graph = frame("old-graph-row-model", "old-row-result", 1, "partial");
  const newer = frame(
    "newer-table-row-model",
    "newer-row-result",
    2,
    "complete",
  );
  const pages = new Map(
    [graph, newer].map((value) => [value.model.modelRef, rowsPage(value)]),
  );
  const visualization = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(graph).mockResolvedValueOnce(newer),
    rows: vi.fn(async (request: VizRowsRequest): Promise<VizRows> => {
      const page = pages.get(request.modelRef)!.response;
      return { ...page, rows: page.rows };
    }),
    cancel: vi.fn(),
    canDisplay: (value) => value.model.modelRef === graph.model.modelRef,
  });
  const frames = new Map([
    [graph.model.modelRef, graph],
    [newer.model.modelRef, newer],
  ]);
  const groupPort = vi.fn(
    async (request: {
      modelRef: string;
      groupRef: string;
      start: number;
      count: number;
    }) => groupPage(frames.get(request.modelRef)!),
  );
  const groups = new GroupController({ rows: groupPort });

  render(
    <ControlledVisualization
      controller={visualization}
      groupController={groups}
      text={fixtureText}
      autoLoadRows
    />,
  );
  await act(async () => visualization.render(renderRequest(graph)));
  fireEvent.click(screen.getByRole("button", { name: "Rows from this graph" }));
  await act(async () => visualization.render(renderRequest(newer)));
  fireEvent.click(screen.getByRole("button", { name: "Items in this result" }));

  const rows = screen.getAllByRole("button", {
    name: "View members of Shared group",
  });
  expect(rows).toHaveLength(2);
  const graphRow = rows[0]!;
  const newerTableRow = rows[1]!;

  pressEnter(graphRow);
  await screen.findByText(`Member from ${graph.model.modelRef}`);
  expect(groupPort).toHaveBeenLastCalledWith({
    modelRef: graph.model.modelRef,
    groupRef: sharedGroup,
    start: 0,
    count: 100,
  });
  pressEnter(screen.getByRole("button", { name: "Back to visualization" }));
  expect(document.activeElement).toBe(graphRow);

  pressEnter(newerTableRow);
  await screen.findByText(`Member from ${newer.model.modelRef}`);
  expect(groupPort).toHaveBeenLastCalledWith({
    modelRef: newer.model.modelRef,
    groupRef: sharedGroup,
    start: 0,
    count: 100,
  });
  pressEnter(screen.getByRole("button", { name: "Back to visualization" }));
  expect(document.activeElement).toBe(newerTableRow);
});
