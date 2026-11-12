import { createChartRenderers } from "../src/components";
import type { GeometryRendererProps } from "../src/components";
import { visualizationMessage } from "../src/features/visualization/messages";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { VisualizationController } from "../src/features/visualization/controller";
import { ControlledVisualization } from "../src/features/visualization/ControlledVisualization";
import { GroupController } from "../src/features/visualization/group-controller";
import type {
  VizRenderRequest,
  VizRenderResponse,
  VizGroupRows,
  VizRowsRequest,
} from "../src/ipc";

afterEach(cleanup);
const original = modelFixture();
const revision = original.frame.resultRevision;
const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
};
const request = (seq: number, version = revision): VizRenderRequest => ({
  viewChannel: "view",
  requestSeq: seq,
  resultRevision: version,
  format: "treemap",
  aggregateScopeRef: { resultVersion: version, itemNo: 0 },
  navigationScopeRef: { resultVersion: version, itemNo: 0 },
  measure: "size",
  sizeBasis: "logical",
  classificationCriterion: "type",
  colorCriterion: "type",
  denominatorBasis: "scope",
  minAreaPx2: 1,
  depthLimit: 2,
  ...view,
});
function frame(seq: number, version = revision) {
  const f = modelFixture().frame;
  f.requestSeq = seq;
  f.resultRevision = version;
  f.model.modelRef = `model-${seq}`;
  f.model.aggregateScopeRef.resultVersion = version;
  f.model.navigationScopeRef.resultVersion = version;
  for (const shape of f.model.shapes) {
    if (shape.itemRef) shape.itemRef.resultVersion = version;
    if (shape.rootBranchRef) shape.rootBranchRef.resultVersion = version;
  }
  return f;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}

test.each(["keyboard", "pointer"] as const)(
  "returns to the actual adapted group shape after %s activation",
  async (activation) => {
    const result = frame(1);
    const shape = result.model.shapes[0]!;
    shape.itemRef = null;
    shape.groupRef = "group-origin";
    const visualization = new VisualizationController({
      render: vi.fn().mockResolvedValue(result),
      rows: vi.fn(),
      cancel: vi.fn(),
      canDisplay: () => true,
    });
    const groups = new GroupController({
      rows: vi.fn().mockResolvedValue({
        rows: [],
        totalCount: 0,
        groupTotal: shape.size,
        sizeRange: { kind: "unknown" },
        resultRevision: result.resultRevision,
        observedAt: result.observedAt,
        completeness: result.completeness,
      } satisfies VizGroupRows),
    });
    render(
      <ControlledVisualization
        controller={visualization}
        groupController={groups}
        text={fixtureText}
      />,
    );
    await act(async () => visualization.render(request(1)));
    const shapeButton = within(
      screen.getByRole("group", { name: "Space distribution" }),
    ).getAllByRole("button")[0]!;
    shapeButton.focus();
    if (activation === "keyboard") {
      fireEvent.keyDown(shapeButton, { key: " ", code: "Space" });
      fireEvent.keyUp(shapeButton, { key: " ", code: "Space" });
    } else fireEvent.click(shapeButton);
    fireEvent.click(screen.getByRole("button", { name: "View group members" }));
    const back = await screen.findByRole("button", {
      name: "Back to visualization",
    });
    expect(
      screen
        .getAllByRole("heading")
        .some((heading) => heading.textContent?.includes("Other members")),
    ).toBe(false);
    back.focus();
    fireEvent.click(back);
    expect(document.activeElement).toBe(shapeButton);
  },
);

test("selection while an unrelated control has focus does not become a shape origin", async () => {
  const result = frame(1);
  const shape = result.model.shapes[0]!;
  shape.itemRef = null;
  shape.groupRef = "group-origin";
  const selection = {
    kind: "group" as const,
    groupRef: "group-origin",
    modelRef: result.model.modelRef,
    resultRevision: result.resultRevision,
  };
  const visualization = new VisualizationController({
    render: vi.fn().mockResolvedValue(result),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const groups = new GroupController({
    rows: vi.fn().mockResolvedValue({
      rows: [],
      totalCount: 0,
      groupTotal: shape.size,
      sizeRange: { kind: "unknown" },
      resultRevision: result.resultRevision,
      observedAt: result.observedAt,
      completeness: result.completeness,
    } satisfies VizGroupRows),
  });
  render(
    <>
      <button>Unrelated control</button>
      <ControlledVisualization
        controller={visualization}
        groupController={groups}
        text={fixtureText}
      />
    </>,
  );
  await act(async () => visualization.render(request(1)));
  const external = screen.getByRole("button", { name: "Unrelated control" });
  external.focus();
  act(() => visualization.select(selection));
  fireEvent.click(screen.getByRole("button", { name: "View group members" }));
  const back = await screen.findByRole("button", {
    name: "Back to visualization",
  });
  back.focus();
  fireEvent.click(back);
  await screen.findByText(
    "The original group target is no longer available. Return to the visualization.",
  );
  expect(document.activeElement?.getAttribute("aria-labelledby")).toBeTruthy();
  expect(document.activeElement).not.toBe(external);
});

test("an injected chart renderer cannot claim a button as a verified shape origin", async () => {
  const result = frame(1);
  const shape = result.model.shapes[0]!;
  shape.itemRef = null;
  shape.groupRef = "group-origin";
  const visualization = new VisualizationController({
    render: vi.fn().mockResolvedValue(result),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const groups = new GroupController({
    rows: vi.fn().mockResolvedValue({
      rows: [],
      totalCount: 0,
      groupTotal: shape.size,
      sizeRange: { kind: "unknown" },
      resultRevision: result.resultRevision,
      observedAt: result.observedAt,
      completeness: result.completeness,
    } satisfies VizGroupRows),
  });
  const ForgedRenderer = (props: GeometryRendererProps) => {
    const rendered = props.shapes[0];
    if (!rendered) return null;
    return (
      <button
        onFocus={() => props.onShapeFocus?.(rendered.rowRef, rendered.shapeRef)}
        onClick={() => props.onSelect(rendered.rowRef, rendered.shapeRef)}
      >
        {rendered.accessibleLabel}
      </button>
    );
  };
  render(
    <ControlledVisualization
      controller={visualization}
      groupController={groups}
      resolveRenderer={createChartRenderers([["treemap", ForgedRenderer]])}
      text={fixtureText}
    />,
  );
  await act(async () => visualization.render(request(1)));
  const fakeShape = within(
    screen.getByRole("region", { name: "Storage visualization" }),
  ).getAllByRole("button")[0]!;
  fakeShape.focus();
  fireEvent.click(fakeShape);
  fireEvent.click(screen.getByRole("button", { name: "View group members" }));
  const back = await screen.findByRole("button", {
    name: "Back to visualization",
  });
  back.focus();
  fireEvent.click(back);
  expect(
    await screen.findByText(
      "The original group target is no longer available. Return to the visualization.",
    ),
  ).toBeTruthy();
  expect(document.activeElement).not.toBe(fakeShape);
});

test("a removed graph group row returns to its rows title with an explanation", async () => {
  const result = frame(1);
  const fixturePage = modelFixture().page;
  const groupRow = {
    ...fixturePage.response.rows[0]!,
    itemRef: null,
    groupRef: "row-group",
    displayName: "Other items",
  };
  const rows = vi
    .fn()
    .mockResolvedValueOnce({
      ...fixturePage.response,
      resultRevision: result.resultRevision,
      rows: [groupRow],
    })
    .mockResolvedValueOnce({
      ...fixturePage.response,
      resultRevision: result.resultRevision,
      rows: [],
    });
  const visualization = new VisualizationController({
    render: vi.fn().mockResolvedValue(result),
    rows,
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const groups = new GroupController({
    rows: vi.fn().mockResolvedValue({
      rows: [],
      totalCount: 0,
      groupTotal: result.model.shapes[0]!.size,
      sizeRange: { kind: "unknown" },
      resultRevision: result.resultRevision,
      observedAt: result.observedAt,
      completeness: result.completeness,
    } satisfies VizGroupRows),
  });
  render(
    <ControlledVisualization
      controller={visualization}
      groupController={groups}
      text={fixtureText}
    />,
  );
  await act(async () => visualization.render(request(1)));
  await act(async () =>
    visualization.rows({
      modelRef: result.model.modelRef,
      resultRevision: result.resultRevision,
      start: 0,
      count: 100,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Rows from this graph" }));
  fireEvent.click(
    screen.getByRole("button", { name: "View members of Other items" }),
  );
  const back = await screen.findByRole("button", {
    name: "Back to visualization",
  });
  await act(async () =>
    visualization.rows({
      modelRef: result.model.modelRef,
      resultRevision: result.resultRevision,
      start: 100,
      count: 100,
    }),
  );
  expect(
    screen.queryByRole("button", { name: "View members of Other items" }),
  ).toBeNull();
  back.focus();
  fireEvent.click(back);
  await screen.findByText(
    "The original group target is no longer available. Return to the visualization.",
  );
  expect(document.activeElement).toBe(
    screen.getByRole("group", { name: "Rows from this graph" }),
  );
});

test("latest request alone commits and current failure retains the last valid graph", async () => {
  const a = deferred<VizRenderResponse>(),
    b = deferred<VizRenderResponse>(),
    c = deferred<VizRenderResponse>();
  const renderPort = vi
    .fn()
    .mockReturnValueOnce(a.promise)
    .mockReturnValueOnce(b.promise)
    .mockReturnValueOnce(c.promise);
  const cancel = vi.fn().mockResolvedValue({ controlResult: "accepted" });
  const controller = new VisualizationController({
    render: renderPort,
    rows: vi.fn(),
    cancel,
    canDisplay: () => true,
  });
  const first = controller.render(request(1));
  const second = controller.render(request(2));
  const third = controller.render(request(3));
  expect(cancel).not.toHaveBeenCalled();
  b.resolve(frame(2));
  a.resolve(frame(1));
  await Promise.all([first, second]);
  expect(controller.getSnapshot().graph).toBeNull();
  c.resolve(frame(3));
  await third;
  expect(controller.getSnapshot().graph?.frame.requestSeq).toBe(3);
  renderPort.mockRejectedValueOnce(new Error("failed"));
  await controller.render(request(4));
  expect(controller.getSnapshot()).toMatchObject({
    phase: "error",
    graph: { frame: { requestSeq: 3 } },
  });
});

test("new unsupported model has its own table while the old graph retains its own rows", async () => {
  const first = frame(1),
    second = frame(2, "new-revision");
  const page = modelFixture().page;
  const rows = vi.fn(async (r: VizRowsRequest) => ({
    ...page.response,
    resultRevision: r.resultRevision,
    rows: [],
  }));
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(second),
    rows,
    cancel: vi.fn().mockResolvedValue({ controlResult: "accepted" }),
    canDisplay: (f) => f.requestSeq === 1,
  });
  await controller.render(request(1));
  await controller.rows({
    modelRef: first.model.modelRef,
    resultRevision: revision,
    start: 0,
    count: 10,
  });
  const graphPage = controller.getSnapshot().graphPage;
  await controller.render(request(2, "new-revision"));
  await controller.rows({
    modelRef: second.model.modelRef,
    resultRevision: "new-revision",
    start: 0,
    count: 10,
  });
  const state = controller.getSnapshot();
  expect(state.graph?.frame).toBe(first);
  expect(state.graphPage).toBe(graphPage);
  expect(state.table).toBe(second);
  expect(state.tablePage?.response.resultRevision).toBe("new-revision");
  await expect(
    controller.rows({
      modelRef: first.model.modelRef,
      resultRevision: revision,
      start: 0,
      count: 10,
    }),
  ).rejects.toThrow();
});

test("inspection stays pinned and only evidenced same or moved item selection and focus transfer", async () => {
  const first = frame(1),
    second = frame(2, "next");
  first.model.shapes[0]!.matchKey = {
    state: "present",
    value: {
      volumeId: "volume",
      pathCompareKey: "path",
      fileId: null,
      identityCapability: { state: "supported" },
    },
  };
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(second),
    rows: vi.fn(),
    cancel: vi.fn().mockResolvedValue({ controlResult: "accepted" }),
    canDisplay: () => true,
  });
  await controller.render(request(1));
  const shape = first.model.shapes[0]!;
  const item = shape.itemRef!;
  controller.select({
    kind: "item",
    itemRef: item,
    matchKey: shape.matchKey,
    modelRef: first.model.modelRef,
    resultRevision: revision,
  });
  controller.focus(item);
  controller.offerRevision("next");
  expect(controller.getSnapshot().graph?.frame.resultRevision).toBe(revision);
  const next = { resultVersion: "next", itemNo: item.itemNo };
  await controller.render(request(2, "next"), [
    { from: item, to: next, outcome: "moved" },
  ]);
  expect(controller.getSnapshot().selection).toMatchObject({
    itemRef: next,
    resultRevision: "next",
  });
  expect(controller.getSnapshot().focus).toEqual(next);
});

test("controller graph adapter renders only the committed model", async () => {
  const f = frame(1);
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValue(f),
    rows: vi.fn(),
    cancel: vi.fn().mockResolvedValue({ controlResult: "accepted" }),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  expect(
    screen.queryByRole("region", { name: "Storage visualization" }),
  ).toBeNull();
  await act(async () => controller.render(request(1)));
  expect(
    screen.getByRole("region", { name: "Storage visualization" }),
  ).toBeTruthy();
});

test("unverified correspondence clears selection and focus, retaining only previous-selection evidence", async () => {
  const first = frame(1),
    next = frame(2, "next");
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(next),
    rows: vi.fn(),
    cancel: vi.fn().mockResolvedValue({ controlResult: "accepted" }),
    canDisplay: () => true,
  });
  await controller.render(request(1));
  const shape = first.model.shapes[0]!;
  const selection = {
    kind: "item" as const,
    itemRef: shape.itemRef!,
    matchKey: shape.matchKey,
    modelRef: first.model.modelRef,
    resultRevision: revision,
  };
  controller.select(selection);
  controller.focus(shape.itemRef);
  await controller.render(request(2, "next"), [
    {
      from: shape.itemRef!,
      to: { resultVersion: "next", itemNo: shape.itemRef!.itemNo },
      outcome: "same",
    },
  ]);
  expect(controller.getSnapshot()).toMatchObject({
    selection: null,
    focus: null,
    previousSelection: selection,
  });
});

test("late rows never attach to a replacement model and job receipts stay pending", async () => {
  const first = frame(1),
    second = frame(2);
  const waiting =
    deferred<ReturnType<typeof modelFixture>["page"]["response"]>();
  const renderPort = vi
    .fn()
    .mockResolvedValueOnce(first)
    .mockResolvedValueOnce(second)
    .mockResolvedValueOnce({
      kind: "job",
      requestSeq: 3,
      resultRevision: revision,
      observedAt: null,
      completeness: "partial",
      jobRef: "job",
    });
  const controller = new VisualizationController({
    render: renderPort,
    rows: () => waiting.promise,
    cancel: vi.fn().mockResolvedValue({ controlResult: "accepted" }),
    canDisplay: () => true,
  });
  await controller.render(request(1));
  const oldRows = controller.rows({
    modelRef: first.model.modelRef,
    resultRevision: revision,
    start: 0,
    count: 5,
  });
  await controller.render(request(2));
  waiting.resolve(modelFixture().page.response);
  await oldRows;
  expect(controller.getSnapshot().tablePage).toBeNull();
  await controller.render(request(3));
  expect(controller.getSnapshot()).toMatchObject({
    phase: "job",
    graph: { frame: { requestSeq: 2 } },
  });
});

test("a rejected replacement preserves the active job without cancelling it", async () => {
  const first = frame(1);
  const receipt = {
    kind: "job" as const,
    requestSeq: 2,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial" as const,
    jobRef: "active-job",
  };
  const cancel = vi.fn();
  const controller = new VisualizationController({
    render: vi
      .fn()
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(receipt)
      .mockRejectedValueOnce(new Error("busy")),
    rows: vi.fn(),
    cancel,
    canDisplay: () => true,
  });
  await controller.render(request(1));
  await controller.render(request(2));
  await controller.render(request(3));
  expect(cancel).not.toHaveBeenCalled();
  expect(controller.getSnapshot()).toMatchObject({
    phase: "error",
    graph: { frame: { requestSeq: 1 } },
    job: receipt,
  });
});

test("entry point shows loading, failure and a newer table independently of the old graph", async () => {
  const first = frame(1),
    newer = frame(2, "new-revision");
  newer.observedAt = "2026-11-11T04:00:00Z";
  const waiting = deferred<VizRenderResponse>();
  const rowWait =
    deferred<ReturnType<typeof modelFixture>["page"]["response"]>();
  const controller = new VisualizationController({
    render: vi
      .fn()
      .mockResolvedValueOnce(first)
      .mockReturnValueOnce(waiting.promise)
      .mockRejectedValueOnce(new Error("busy")),
    rows: () => rowWait.promise,
    cancel: vi.fn(),
    canDisplay: (f) => f.requestSeq === 1,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  await act(async () => controller.render(request(1)));
  let pending!: Promise<void>;
  act(() => {
    pending = controller.render(request(2, "new-revision"));
  });
  expect(
    screen.getByRole("progressbar", { name: "Loading visualization" }),
  ).toBeTruthy();
  await act(async () => {
    waiting.resolve(newer);
    await pending;
  });
  expect(
    screen.getByRole("region", { name: "Storage visualization" }),
  ).toBeTruthy();
  expect(
    screen.getByRole("region", { name: "Items in this result" }),
  ).toBeTruthy();
  let rows!: Promise<void>;
  act(() => {
    rows = controller.rows({
      modelRef: newer.model.modelRef,
      resultRevision: "new-revision",
      start: 0,
      count: 10,
    });
  });
  expect(
    screen.getByRole("progressbar", { name: "Loading items in this result" }),
  ).toBeTruthy();
  await act(async () => {
    rowWait.resolve({
      ...modelFixture().page.response,
      resultRevision: "new-revision",
      observedAt: newer.observedAt,
      rows: [],
    });
    await rows;
  });
  fireEvent.click(screen.getByRole("button", { name: /Items in this result/ }));
  expect(screen.getByText("No rows in this page")).toBeTruthy();
  await act(async () => controller.render(request(3, "new-revision")));
  expect(screen.getByText("Visualization could not be updated.")).toBeTruthy();
  expect(
    screen.getByRole("region", { name: "Items in this result" }),
  ).toBeTruthy();
});

test("selection is not rebound to absent or ambiguous target shapes", async () => {
  const first = frame(1),
    ambiguous = frame(2);
  const selected = first.model.shapes[0]!;
  ambiguous.model.shapes.push({ ...ambiguous.model.shapes[0]! });
  const controller = new VisualizationController({
    render: vi
      .fn()
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(ambiguous),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  await controller.render(request(1));
  const selection = {
    kind: "item" as const,
    itemRef: selected.itemRef!,
    matchKey: selected.matchKey,
    modelRef: first.model.modelRef,
    resultRevision: revision,
  };
  controller.select(selection);
  await controller.render(request(2));
  expect(controller.getSnapshot()).toMatchObject({
    selection: null,
    previousSelection: selection,
  });
});

test("failed rows keep the visible page and offer retry for the same model and range", async () => {
  const f = frame(1);
  const page = modelFixture().page;
  const rows = vi
    .fn()
    .mockResolvedValueOnce({ ...page.response, rows: [] })
    .mockRejectedValueOnce(new Error("page failed"))
    .mockResolvedValueOnce({ ...page.response, rows: [] });
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValue(f),
    rows,
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  await act(async () => controller.render(request(1)));
  const first = {
    modelRef: f.model.modelRef,
    resultRevision: revision,
    start: 0,
    count: 5,
  };
  const failed = { ...first, start: 5 };
  await act(async () => controller.rows(first));
  const previous = controller.getSnapshot().tablePage;
  await act(async () => controller.rows(failed));
  expect(controller.getSnapshot().tablePage).toBe(previous);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Retry rows" }));
  });
  expect(rows).toHaveBeenLastCalledWith(failed);
});

test("accepted unavailable clears the previous job while rejected admission preserves it", async () => {
  const first = frame(1);
  const job = {
    kind: "job" as const,
    requestSeq: 2,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial" as const,
    jobRef: "job",
  };
  const unavailable = {
    kind: "unavailable" as const,
    requestSeq: 4,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial" as const,
    reasonKey: "viz.unavailable.no_baseline",
    availableFormats: [],
    fallback: null,
  };
  const cancel = vi.fn();
  const controller = new VisualizationController({
    render: vi
      .fn()
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(job)
      .mockRejectedValueOnce(new Error("busy"))
      .mockResolvedValueOnce(unavailable),
    rows: vi.fn(),
    cancel,
    canDisplay: () => true,
  });
  await controller.render(request(1));
  await controller.render(request(2));
  await controller.render(request(3));
  expect(controller.getSnapshot()).toMatchObject({ phase: "error", job });
  await controller.render(request(4));
  expect(controller.getSnapshot()).toMatchObject({
    phase: "unavailable",
    job: null,
    response: unavailable,
    graph: { frame: { requestSeq: 1 } },
  });
  await controller.cancelCurrent();
  expect(cancel).not.toHaveBeenCalled();
});

test("the newer fallback table loads its first page from the UI without changing the old graph", async () => {
  const first = frame(1),
    newer = frame(2, "new-revision");
  const original = modelFixture().page.response;
  const rows = vi.fn(async (r: VizRowsRequest) => ({
    ...original,
    resultRevision: r.resultRevision,
    rows: [
      {
        ...original.rows[0]!,
        itemRef: { resultVersion: r.resultRevision, itemNo: 1 },
        displayName: r.resultRevision === revision ? "Old item" : "New item",
      },
    ],
  }));
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(newer),
    rows,
    cancel: vi.fn(),
    canDisplay: (f) => f.requestSeq === 1,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  await act(async () => controller.render(request(1)));
  await act(async () =>
    controller.rows({
      modelRef: first.model.modelRef,
      resultRevision: revision,
      start: 0,
      count: 100,
    }),
  );
  await act(async () => controller.render(request(2, "new-revision")));
  const graph = screen.getByRole("region", { name: "Storage visualization" });
  expect(within(graph).getAllByText(/Old item/).length).toBeGreaterThan(0);
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Load items in this result" }),
    );
  });
  expect(rows).toHaveBeenLastCalledWith({
    modelRef: newer.model.modelRef,
    resultRevision: "new-revision",
    start: 0,
    count: 100,
  });
  const table = screen.getByRole("region", { name: "Items in this result" });
  fireEvent.click(
    within(table).getByRole("button", { name: /Items in this result/ }),
  );
  expect(within(table).getByText("New item")).toBeTruthy();
  expect(within(graph).queryByText(/New item/)).toBeNull();
  expect(controller.getSnapshot().graphPage?.request.modelRef).toBe(
    first.model.modelRef,
  );
});

test.each([false, true])(
  "row retry retains focus during pending and respects subsequent focus movement: %s",
  async (moveAway) => {
    const f = frame(1);
    const page = modelFixture().page.response;
    const wait = deferred<typeof page>();
    const rows = vi
      .fn()
      .mockRejectedValueOnce(new Error("failed"))
      .mockReturnValueOnce(wait.promise);
    const controller = new VisualizationController({
      render: vi.fn().mockResolvedValue(f),
      rows,
      cancel: vi.fn(),
      canDisplay: () => true,
    });
    render(
      <>
        <button>Other action</button>
        <ControlledVisualization controller={controller} text={fixtureText} />
      </>,
    );
    await act(async () => controller.render(request(1)));
    await act(async () =>
      controller.rows({
        modelRef: f.model.modelRef,
        resultRevision: revision,
        start: 0,
        count: 5,
      }),
    );
    const retry = screen.getByRole("button", { name: "Retry rows" });
    retry.focus();
    act(() => fireEvent.click(retry));
    const pending = screen.getByRole("button", { name: "Retrying rows…" });
    expect(document.activeElement).toBe(pending);
    expect(pending.getAttribute("aria-disabled")).toBe("true");
    act(() => fireEvent.click(pending));
    expect(rows).toHaveBeenCalledTimes(2);
    const other = screen.getByRole("button", { name: "Other action" });
    if (moveAway) other.focus();
    await act(async () => {
      wait.resolve({ ...page, rows: [] });
      await wait.promise;
    });
    expect(document.activeElement).toBe(
      moveAway ? other : screen.getByText("Rows for the displayed graph"),
    );
  },
);

test("keyboard focus and selection follow verified correspondence into the new DOM target", async () => {
  const first = frame(1),
    next = frame(2, "next");
  first.model.shapes[0]!.matchKey = {
    state: "present",
    value: {
      volumeId: "volume",
      pathCompareKey: "path",
      fileId: null,
      identityCapability: { state: "supported" },
    },
  };
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(next),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  await act(async () => controller.render(request(1)));
  const shape = first.model.shapes[0]!;
  const item = shape.itemRef!;
  const group = screen.getByRole("group", { name: "Space distribution" });
  const button = within(group).getAllByRole("button")[0]!;
  act(() => {
    button.focus();
    fireEvent.keyDown(button, { key: " ", code: "Space" });
    fireEvent.keyUp(button, { key: " ", code: "Space" });
  });
  expect(controller.getSnapshot().focus).toEqual(item);
  expect(controller.getSnapshot().selection).toMatchObject({
    kind: "item",
    itemRef: item,
  });
  const target = { resultVersion: "next", itemNo: item.itemNo };
  await act(async () =>
    controller.render(request(2, "next"), [
      { from: item, to: target, outcome: "moved" },
    ]),
  );
  const newButton = within(
    screen.getByRole("group", { name: "Space distribution" }),
  ).getAllByRole("button")[0]!;
  expect(document.activeElement).toBe(newButton);
  expect(controller.getSnapshot().focus).toEqual(target);
});

test("status exposes real unavailable reason and revision callback without inventing correspondence", async () => {
  const f = frame(1);
  const unavailable = {
    kind: "unavailable" as const,
    requestSeq: 2,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial" as const,
    reasonKey: "viz.unavailable.no_baseline",
    availableFormats: ["bar"],
    fallback: null,
  };
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(f).mockResolvedValueOnce(unavailable),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const apply = vi.fn();
  render(
    <ControlledVisualization
      controller={controller}
      text={fixtureText}
      onApplyRevision={apply}
    />,
  );
  await act(async () => controller.render(request(1)));
  act(() => controller.offerRevision("next"));
  fireEvent.click(screen.getByRole("button", { name: "Review newer result" }));
  expect(apply).toHaveBeenCalledWith("next");
  expect(controller.getSnapshot().graph?.frame.resultRevision).toBe(revision);
  await act(async () => controller.render(request(2)));
  expect(screen.getByText(/No comparison baseline is available/)).toBeTruthy();
  expect(screen.getByText(/Available formats/).textContent).toContain(
    "Bar chart",
  );
});

test("lost correspondence is explained and never moves focus to another application control", async () => {
  const first = frame(1),
    next = frame(2, "next");
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(next),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <>
      <button>Outside graph</button>
      <ControlledVisualization controller={controller} text={fixtureText} />
    </>,
  );
  await act(async () => controller.render(request(1)));
  const graphButton = within(
    screen.getByRole("group", { name: "Space distribution" }),
  ).getAllByRole("button")[0]!;
  act(() => {
    graphButton.focus();
    fireEvent.keyDown(graphButton, { key: " ", code: "Space" });
    fireEvent.keyUp(graphButton, { key: " ", code: "Space" });
  });
  const outside = screen.getByRole("button", { name: "Outside graph" });
  outside.focus();
  await act(async () => controller.render(request(2, "next")));
  expect(document.activeElement).toBe(outside);
  expect(
    screen.getByText(/previous selection could not be matched/),
  ).toBeTruthy();
});

test("retry visualization repeats only an explicit failed request with the next sequence", async () => {
  const renderPort = vi
    .fn()
    .mockRejectedValueOnce(new Error("busy"))
    .mockResolvedValueOnce(frame(2));
  const controller = new VisualizationController({
    render: renderPort,
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  await act(async () => controller.render(request(1)));
  expect(renderPort).toHaveBeenCalledTimes(1);
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Retry visualization" }),
    );
  });
  expect(renderPort).toHaveBeenLastCalledWith(request(2));
  expect(controller.getSnapshot().graph?.frame.requestSeq).toBe(2);
  expect(controller.nextRequestSequence()).toBe(3);
});

test("opt-in row prefetch uses the committed model and revision once", async () => {
  const current = frame(1);
  const rows = vi.fn().mockResolvedValue({
    ...modelFixture().page.response,
    resultRevision: revision,
    rows: [],
  });
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValue(current),
    rows,
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization
      controller={controller}
      text={fixtureText}
      autoLoadRows
    />,
  );
  await act(async () => controller.render(request(1)));
  await waitFor(() => expect(rows).toHaveBeenCalledTimes(1));
  expect(rows).toHaveBeenCalledWith({
    modelRef: current.model.modelRef,
    resultRevision: revision,
    start: 0,
    count: 100,
  });
});

test.each(["success", "failure", "move-away"] as const)(
  "render retry keeps or returns focus for %s",
  async (outcome) => {
    const response = deferred<VizRenderResponse>();
    const port = vi
      .fn()
      .mockRejectedValueOnce(new Error("busy"))
      .mockReturnValueOnce(response.promise);
    const controller = new VisualizationController({
      render: port,
      rows: vi.fn(),
      cancel: vi.fn(),
      canDisplay: () => true,
    });
    render(
      <>
        <button>Outside</button>
        <ControlledVisualization controller={controller} text={fixtureText} />
      </>,
    );
    await act(async () => controller.render(request(1)));
    const retry = screen.getByRole("button", { name: "Retry visualization" });
    retry.focus();
    act(() => fireEvent.click(retry));
    const pending = screen.getByRole("button", {
      name: "Retrying visualization…",
    });
    expect(document.activeElement).toBe(pending);
    expect(pending.getAttribute("aria-disabled")).toBe("true");
    act(() => fireEvent.click(pending));
    expect(port).toHaveBeenCalledTimes(2);
    const outside = screen.getByRole("button", { name: "Outside" });
    if (outcome === "move-away") outside.focus();
    await act(async () => {
      if (outcome === "failure") response.reject(new Error("still busy"));
      else response.resolve(frame(2));
      await response.promise.catch(() => {});
    });
    if (outcome === "failure")
      expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "Retry visualization" }),
      );
    else if (outcome === "move-away")
      expect(document.activeElement).toBe(outside);
    else
      expect(document.activeElement).toBe(
        screen.getByText("Visualization result updated."),
      );
  },
);

test("Korean visualization status uses Korean reason and format names", async () => {
  const unavailable = {
    kind: "unavailable" as const,
    requestSeq: 1,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial" as const,
    reasonKey: "viz.unavailable.no_baseline",
    availableFormats: ["viz.bar", "treemap"],
    fallback: null,
  };
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValue(unavailable),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const text = {
    ...fixtureText,
    message: (key: Parameters<typeof fixtureText.message>[0]) =>
      visualizationMessage("ko", key),
  };
  render(<ControlledVisualization controller={controller} text={text} />);
  await act(async () => controller.render(request(1)));
  expect(screen.getByText(/비교 기준이 없습니다/)).toBeTruthy();
  expect(screen.getByText(/사용 가능한 형식/).textContent).toContain(
    "막대 그래프, 트리맵",
  );
});

test.each(["unknown", "duplicate", "outside"] as const)(
  "unmatched graph focus has a safe destination for %s",
  async (scenario) => {
    const first = frame(1),
      next = frame(2, "next");
    first.model.shapes[0]!.matchKey = {
      state: "present",
      value: {
        volumeId: "volume",
        pathCompareKey: "path",
        fileId: null,
        identityCapability: { state: "supported" },
      },
    };
    const controller = new VisualizationController({
      render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(next),
      rows: vi.fn(),
      cancel: vi.fn(),
      canDisplay: () => true,
    });
    render(
      <>
        <button>External</button>
        <ControlledVisualization controller={controller} text={fixtureText} />
      </>,
    );
    await act(async () => controller.render(request(1)));
    const old = within(
      screen.getByRole("group", { name: "Space distribution" }),
    ).getAllByRole("button")[0]!;
    act(() => {
      old.focus();
      fireEvent.keyDown(old, { key: " ", code: "Space" });
      fireEvent.keyUp(old, { key: " ", code: "Space" });
    });
    const external = screen.getByRole("button", { name: "External" });
    if (scenario === "outside") external.focus();
    const item = first.model.shapes[0]!.itemRef!;
    const match = {
      from: item,
      to: { resultVersion: "next", itemNo: item.itemNo },
      outcome: "same" as const,
    };
    const evidence = scenario === "duplicate" ? [match, match] : [];
    await act(async () => controller.render(request(2, "next"), evidence));
    if (scenario === "outside") expect(document.activeElement).toBe(external);
    else
      expect(document.activeElement).toBe(
        screen.getByText(
          "The previously focused item is unavailable in this graph.",
        ),
      );
    expect(controller.getSnapshot().selection).toBeNull();
    expect(
      screen.getByText(
        "The previous selection could not be matched to this result.",
      ),
    ).toBeTruthy();
    const current = within(
      screen.getByRole("group", { name: "Space distribution" }),
    ).getAllByRole("button")[0]!;
    expect(current).not.toBe(old);
    expect(document.activeElement).not.toBe(current);
  },
);

test.each([
  ["en", false],
  ["en", true],
  ["ko", false],
  ["ko", true],
] as const)(
  "explains retained graph and separate model without an offered revision in %s; same revision: %s",
  async (language, sameRevision) => {
    const first = frame(1),
      second = frame(2, sameRevision ? revision : "other-revision");
    first.observedAt = "2026-11-11T01:00:00Z";
    second.observedAt = sameRevision
      ? first.observedAt
      : "2026-11-11T02:00:00Z";
    const controller = new VisualizationController({
      render: vi
        .fn()
        .mockResolvedValueOnce(first)
        .mockResolvedValueOnce(second),
      rows: vi.fn(),
      cancel: vi.fn(),
      canDisplay: (f) => f.requestSeq === 1,
    });
    const text = {
      ...fixtureText,
      message: (key: Parameters<typeof fixtureText.message>[0]) =>
        visualizationMessage(language, key),
    };
    render(<ControlledVisualization controller={controller} text={text} />);
    await act(async () => controller.render(request(1)));
    expect(
      screen.queryByText(
        new RegExp(
          language === "ko"
            ? "이전 그래프를 유지"
            : "previous graph remains displayed",
        ),
      ),
    ).toBeNull();
    await act(async () =>
      controller.render(request(2, sameRevision ? revision : "other-revision")),
    );
    expect(controller.getSnapshot().offeredRevision).toBeNull();
    const graphNotice = screen.getByText(
      new RegExp(
        language === "ko"
          ? "이전 그래프를 유지"
          : "previous graph remains displayed",
      ),
    );
    expect(graphNotice.textContent).toContain(first.observedAt);
    const table = screen.getByRole("region", {
      name: language === "ko" ? "이번 결과의 항목" : "Items in this result",
    });
    expect(table.textContent).toContain(second.observedAt);
    expect(table.textContent).toContain(
      text.message(sameRevision ? "sameRevision" : "differentRevision"),
    );
  },
);

test.each(["en", "ko"] as const)(
  "lost selection shows only its verified previous observation in %s",
  async (language) => {
    const first = frame(1),
      second = frame(2, "new-result");
    first.observedAt = "2026-11-11T01:00:00Z";
    const source = modelFixture().page.response;
    const row = { ...source.rows[0]!, displayName: "Observed directory" };
    const controller = new VisualizationController({
      render: vi
        .fn()
        .mockResolvedValueOnce(first)
        .mockResolvedValueOnce(second),
      rows: vi.fn().mockResolvedValue({
        ...source,
        observedAt: first.observedAt,
        rows: [row],
      }),
      cancel: vi.fn(),
      canDisplay: () => true,
    });
    const text = {
      ...fixtureText,
      message: (key: Parameters<typeof fixtureText.message>[0]) =>
        visualizationMessage(language, key),
    };
    const distributionName =
      language === "ko" ? "공간 분포" : "Space distribution";
    render(<ControlledVisualization controller={controller} text={text} />);
    await act(async () => controller.render(request(1)));
    await act(async () =>
      controller.rows({
        modelRef: first.model.modelRef,
        resultRevision: revision,
        start: 0,
        count: 10,
      }),
    );
    const button = within(
      screen.getByRole("group", { name: distributionName }),
    ).getAllByRole("button")[0]!;
    act(() => {
      button.focus();
      fireEvent.keyDown(button, { key: " ", code: "Space" });
      fireEvent.keyUp(button, { key: " ", code: "Space" });
    });
    await act(async () => controller.render(request(2, "new-result")));
    expect(controller.getSnapshot().previousSelectionEvidence).toMatchObject({
      name: "Observed directory",
      observedAt: first.observedAt,
      sizeBasis: "logical",
    });
    expect(
      screen.getByText(
        new RegExp(language === "ko" ? "이전 항목:" : "Previous item:"),
      ).textContent,
    ).toContain("Observed directory");
    expect(
      screen.getByText(
        new RegExp(language === "ko" ? "이전 관측:" : "Previous observation:"),
      ).textContent,
    ).toContain(first.observedAt);
    expect(
      screen.getByText(text.message("correspondenceUnknown")),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: text.message("focusGraphAction") }),
    ).toBeTruthy();
  },
);

test("duplicate source rows do not establish a previous item name", async () => {
  const first = frame(1),
    second = frame(2, "new-result");
  const source = modelFixture().page.response;
  const row = { ...source.rows[0]!, displayName: "Unreliable duplicate" };
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(second),
    rows: vi.fn().mockResolvedValue({ ...source, rows: [row, row] }),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  await act(async () => controller.render(request(1)));
  await act(async () =>
    controller.rows({
      modelRef: first.model.modelRef,
      resultRevision: revision,
      start: 0,
      count: 10,
    }),
  );
  const shape = first.model.shapes[0]!;
  act(() =>
    controller.select({
      kind: "item",
      itemRef: shape.itemRef!,
      matchKey: shape.matchKey,
      modelRef: first.model.modelRef,
      resultRevision: revision,
    }),
  );
  await act(async () => controller.render(request(2, "new-result")));
  expect(controller.getSnapshot().previousSelectionEvidence?.name).toBeNull();
  expect(screen.queryByText(/Previous item:/)).toBeNull();
});

test.each(["en", "ko"] as const)(
  "unmatched focus reaches the actual graph reselection control in %s",
  async (language) => {
    const first = frame(1),
      next = frame(2, "next");
    const controller = new VisualizationController({
      render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(next),
      rows: vi.fn(),
      cancel: vi.fn(),
      canDisplay: () => true,
    });
    const text = {
      ...fixtureText,
      message: (key: Parameters<typeof fixtureText.message>[0]) =>
        visualizationMessage(language, key),
    };
    const distributionName =
      language === "ko" ? "공간 분포" : "Space distribution";
    render(
      <>
        <ControlledVisualization controller={controller} text={text} />
        <button>Next product control</button>
      </>,
    );
    await act(async () => controller.render(request(1)));
    const old = within(
      screen.getByRole("group", { name: distributionName }),
    ).getAllByRole("button")[0]!;
    act(() => {
      old.focus();
      fireEvent.keyDown(old, { key: " ", code: "Space" });
      fireEvent.keyUp(old, { key: " ", code: "Space" });
    });
    await act(async () => controller.render(request(2, "next")));
    const notice = screen.getByText(text.message("focusLost"));
    const action = screen.getByRole("button", {
      name: text.message("focusGraphAction"),
    });
    expect(document.activeElement).toBe(notice);
    expect(
      notice.compareDocumentPosition(action) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    act(() => fireEvent.click(action));
    const target = within(
      screen.getByRole("group", { name: distributionName }),
    ).getAllByRole("button")[0]!;
    expect(document.activeElement).toBe(target);
    expect(controller.getSnapshot().previousSelection).not.toBeNull();
    act(() => {
      fireEvent.keyDown(target, { key: " ", code: "Space" });
      fireEvent.keyUp(target, { key: " ", code: "Space" });
    });
    expect(controller.getSnapshot().selection).toMatchObject({
      kind: "item",
      resultRevision: "next",
    });
  },
);

test("an empty graph offers no reselection action and leaves the next actual control in document order", async () => {
  const first = frame(1),
    next = frame(2, "next");
  next.model.shapes = [];
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(next),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <>
      <ControlledVisualization controller={controller} text={fixtureText} />
      <button>Next product control</button>
    </>,
  );
  await act(async () => controller.render(request(1)));
  const old = within(
    screen.getByRole("group", { name: "Space distribution" }),
  ).getAllByRole("button")[0]!;
  act(() => {
    old.focus();
    fireEvent.keyDown(old, { key: " ", code: "Space" });
    fireEvent.keyUp(old, { key: " ", code: "Space" });
  });
  await act(async () => controller.render(request(2, "next")));
  const notice = screen.getByText(fixtureText.message("focusLost"));
  expect(document.activeElement).toBe(notice);
  expect(
    screen.queryByRole("button", {
      name: fixtureText.message("focusGraphAction"),
    }),
  ).toBeNull();
  expect(screen.getByText(fixtureText.message("noGraphTargets"))).toBeTruthy();
  const nextControl = screen.getByRole("button", {
    name: "Next product control",
  });
  const following = Array.from(
    document.querySelectorAll("button:not([disabled])"),
  ).filter((button) =>
    Boolean(
      notice.compareDocumentPosition(button) & Node.DOCUMENT_POSITION_FOLLOWING,
    ),
  );
  expect(following[0]).toBe(nextControl);
});

test("ambiguous visible targets do not create a fake reselection action", async () => {
  const first = frame(1),
    next = frame(2, "next");
  next.model.shapes = [
    next.model.shapes[0]!,
    { ...next.model.shapes[0]!, shapeRef: "duplicate-shape" },
  ];
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(next),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  await act(async () => controller.render(request(1)));
  const old = within(
    screen.getByRole("group", { name: "Space distribution" }),
  ).getAllByRole("button")[0]!;
  act(() => {
    old.focus();
    fireEvent.keyDown(old, { key: " ", code: "Space" });
    fireEvent.keyUp(old, { key: " ", code: "Space" });
  });
  await act(async () => controller.render(request(2, "next")));
  expect(document.activeElement).toBe(
    screen.getByText(fixtureText.message("focusLost")),
  );
  expect(
    screen.queryByRole("button", {
      name: fixtureText.message("focusGraphAction"),
    }),
  ).toBeNull();
  expect(screen.getByText(fixtureText.message("noGraphTargets"))).toBeTruthy();
});

test("registered chart renderer receives the actual model while unknown formats use common geometry", async () => {
  const first = frame(1);
  first.model.representation.format = "custom";
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValue(first),
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const Custom = () => (
    <div data-testid="registered-chart">Registered chart</div>
  );
  const resolver = createChartRenderers([["custom", Custom]]);
  const { rerender } = render(
    <ControlledVisualization
      controller={controller}
      text={fixtureText}
      resolveRenderer={resolver}
    />,
  );
  await act(async () => controller.render(request(1)));
  expect(screen.getByTestId("registered-chart")).toBeTruthy();
  rerender(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  expect(
    screen.getByRole("region", { name: "Space distribution" }),
  ).toBeTruthy();
});

test("completed matching visualization job re-queries the same pinned request exactly once", async () => {
  const receipt = {
    kind: "job" as const,
    requestSeq: 1,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial" as const,
    jobRef: "job",
  };
  const port = vi
    .fn()
    .mockResolvedValueOnce(receipt)
    .mockResolvedValueOnce(frame(2));
  const controller = new VisualizationController({
    render: port,
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  await controller.render(request(1));
  const status = {
    jobRef: "job",
    kind: "viz.compute",
    state: "done" as const,
    sequence: 2,
    resultVersion: null,
    reasonKey: null,
  };
  await controller.completeJob({ ...status, jobRef: "other" });
  await controller.completeJob({ ...status, kind: "scan.metadata" });
  expect(port).toHaveBeenCalledTimes(1);
  await controller.completeJob(status);
  await controller.completeJob(status);
  expect(port).toHaveBeenCalledTimes(2);
  expect(port).toHaveBeenLastCalledWith(request(2));
  expect(controller.getSnapshot().graph?.frame.requestSeq).toBe(2);
});

test("completion from an older job cannot replace a newer request or revive a cancelled job", async () => {
  const receipt = {
    kind: "job" as const,
    requestSeq: 1,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial" as const,
    jobRef: "old-job",
  };
  const port = vi
    .fn()
    .mockResolvedValueOnce(receipt)
    .mockResolvedValueOnce(frame(2));
  const controller = new VisualizationController({
    render: port,
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  await controller.render(request(1));
  await controller.render(request(2));
  await controller.completeJob({
    jobRef: "old-job",
    kind: "viz.compute",
    state: "done",
    sequence: 2,
    resultVersion: null,
    reasonKey: null,
  });
  expect(port).toHaveBeenCalledTimes(2);
  expect(controller.getSnapshot().graph?.frame.requestSeq).toBe(2);
});

test("a cancelled visualization job retains its prior graph and does not retry", async () => {
  const first = frame(1);
  const receipt = {
    kind: "job" as const,
    requestSeq: 2,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial" as const,
    jobRef: "job",
  };
  const port = vi
    .fn()
    .mockResolvedValueOnce(first)
    .mockResolvedValueOnce(receipt);
  const controller = new VisualizationController({
    render: port,
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization controller={controller} text={fixtureText} />,
  );
  await act(async () => controller.render(request(1)));
  await act(async () => controller.render(request(2)));
  await act(async () =>
    controller.completeJob({
      jobRef: "job",
      kind: "viz.compute",
      state: "cancelled",
      sequence: 2,
      resultVersion: null,
      reasonKey: null,
    }),
  );
  expect(port).toHaveBeenCalledTimes(2);
  expect(controller.getSnapshot()).toMatchObject({
    phase: "cancelled",
    graph: { frame: { requestSeq: 1 } },
  });
  expect(
    screen.getAllByText(fixtureText.message("cancelled")).length,
  ).toBeGreaterThan(0);
});
