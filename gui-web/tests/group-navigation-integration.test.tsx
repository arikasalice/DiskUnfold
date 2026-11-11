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

afterEach(cleanup);

const source = modelFixture();
const revision = source.frame.resultRevision;
const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 },
};

function request(requestSeq: number): VizRenderRequest {
  return {
    viewChannel: "group-navigation-test",
    requestSeq,
    resultRevision: revision,
    format: "treemap",
    aggregateScopeRef: { resultVersion: revision, itemNo: 0 },
    navigationScopeRef: { resultVersion: revision, itemNo: 0 },
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

function model(modelRef: string, requestSeq: number) {
  const frame = modelFixture().frame;
  frame.requestSeq = requestSeq;
  frame.model.modelRef = modelRef;
  frame.model.shapes = [];
  return frame;
}

function tablePage(modelRef: string, groupRef: string, displayName: string) {
  const page = modelFixture().page;
  page.request.modelRef = modelRef;
  page.request.count = 100;
  page.response.rows = [
    {
      ...page.response.rows[0]!,
      itemRef: null,
      groupRef,
      displayName,
    },
  ];
  page.response.totalCount = 1;
  return page;
}

function groupPage(
  name: string,
  frame: ReturnType<typeof model>,
): VizGroupRows {
  const row = modelFixture().page.response.rows[0]!;
  return {
    rows: [{ ...row, itemRef: row.itemRef, groupRef: null, displayName: name }],
    totalCount: 1,
    groupTotal: row.size,
    sizeRange: {
      kind: "known",
      min: row.size,
      max: row.size,
    },
    resultRevision: frame.resultRevision,
    observedAt: frame.observedAt,
    completeness: frame.completeness,
  };
}

test("fallback table group keyboard entry returns to its row and never restores a replaced row", async () => {
  const first = model("group-model-a", 1);
  const second = model("group-model-b", 2);
  const rowPages = new Map<string, ReturnType<typeof tablePage>>([
    [
      first.model.modelRef,
      tablePage(first.model.modelRef, "group-a", "Group A"),
    ],
    [
      second.model.modelRef,
      tablePage(second.model.modelRef, "group-b", "Group B"),
    ],
  ]);
  const rowPort = vi.fn(async (input: VizRowsRequest): Promise<VizRows> => {
    const page = rowPages.get(input.modelRef)!;
    return {
      ...page.response,
      rows: page.response.rows,
      resultRevision: input.resultRevision,
      observedAt: (input.modelRef === first.model.modelRef ? first : second)
        .observedAt,
      completeness: (input.modelRef === first.model.modelRef ? first : second)
        .completeness,
    };
  });
  const renderPort = vi
    .fn()
    .mockResolvedValueOnce(first)
    .mockResolvedValueOnce(second);
  const visualization = new VisualizationController({
    render: renderPort,
    rows: rowPort,
    cancel: vi.fn(),
    canDisplay: () => false,
  });
  const groupPort = vi.fn(
    async (input: {
      modelRef: string;
      groupRef: string;
      start: number;
      count: number;
    }) =>
      groupPage(
        `Member of ${input.groupRef}`,
        input.groupRef === "group-a" ? first : second,
      ),
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
  await act(async () => visualization.render(request(1)));
  const fallback = screen.getByRole("button", {
    name: "Items in this result",
  });
  fireEvent.click(fallback);
  const firstOrigin = screen.getByRole("button", {
    name: /View members of Group [AB]/,
  });
  firstOrigin.focus();
  fireEvent.keyDown(firstOrigin, { key: "Enter" });
  fireEvent.keyUp(firstOrigin, { key: "Enter" });
  expect(groupPort).toHaveBeenCalled();
  expect(groups.getSnapshot()).toMatchObject({
    frame: { modelRef: first.model.modelRef },
    groupRef: "group-a",
  });
  await screen.findByRole("button", { name: "Back to visualization" });
  expect(groupPort).toHaveBeenLastCalledWith({
    modelRef: first.model.modelRef,
    groupRef: "group-a",
    start: 0,
    count: 100,
  });

  const back = screen.getByRole("button", { name: "Back to visualization" });
  back.focus();
  fireEvent.keyDown(back, { key: "Enter" });
  fireEvent.keyUp(back, { key: "Enter" });
  expect(document.activeElement).toBe(firstOrigin);

  firstOrigin.focus();
  fireEvent.keyDown(firstOrigin, { key: "Enter" });
  fireEvent.keyUp(firstOrigin, { key: "Enter" });
  await screen.findByRole("button", { name: "Back to visualization" });
  expect(groupPort).toHaveBeenCalledTimes(2);
  expect(groupPort).toHaveBeenLastCalledWith({
    modelRef: first.model.modelRef,
    groupRef: "group-a",
    start: 0,
    count: 100,
  });
  await act(async () => visualization.render(request(2)));
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: "Back to visualization" }),
    ).toBeNull(),
  );
  expect(document.activeElement).not.toBe(firstOrigin);

  fireEvent.click(screen.getByRole("button", { name: "Items in this result" }));
  const secondOrigin = screen.getByRole("button", {
    name: /View members of Group [AB]/,
  });
  secondOrigin.focus();
  fireEvent.keyDown(secondOrigin, { key: "Enter" });
  fireEvent.keyUp(secondOrigin, { key: "Enter" });
  await screen.findByText("Member of group-b");
  expect(groupPort).toHaveBeenLastCalledWith({
    modelRef: second.model.modelRef,
    groupRef: "group-b",
    start: 0,
    count: 100,
  });
  fireEvent.click(
    screen.getByRole("button", { name: "Back to visualization" }),
  );
  expect(document.activeElement).toBe(secondOrigin);
});
