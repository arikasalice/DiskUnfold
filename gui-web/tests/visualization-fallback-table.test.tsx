import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { ControlledVisualization } from "../src/features/visualization/ControlledVisualization";
import { VisualizationController } from "../src/features/visualization/controller";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizRenderRequest, VizRenderResponse, VizRows } from "../src/ipc";

afterEach(cleanup);

const text = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("en", key),
    ),
  { system: "iec", unit: "auto" },
);
const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
};
const revision = modelFixture().frame.resultRevision;
const request = (seq: number): VizRenderRequest => ({
  viewChannel: "view",
  requestSeq: seq,
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
});
function model(seq: number): VizRenderResponse {
  const f = modelFixture().frame;
  f.requestSeq = seq;
  f.model.modelRef = `model-${seq}`;
  return f;
}
const unavailable = (
  seq: number,
  fallback: { modelRef: string; resultRevision: string } | null,
): VizRenderResponse => ({
  kind: "unavailable",
  requestSeq: seq,
  resultRevision: revision,
  observedAt: null,
  completeness: "complete",
  reasonKey: "viz.unavailable.no_attribution",
  availableFormats: [],
  fallback,
});
const fallbackRows = (): VizRows => {
  const { page } = modelFixture();
  return { ...page.response, resultRevision: revision };
};
const firstName = () => fallbackRows().rows[0]!.displayName;
function setup(renders: VizRenderResponse[], rows = vi.fn()) {
  const renderPort = vi.fn();
  for (const answer of renders) renderPort.mockResolvedValueOnce(answer);
  const controller = new VisualizationController({
    render: renderPort,
    rows,
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const shown = render(
    <ControlledVisualization controller={controller} text={text} />,
  );
  return { controller, rows, ...shown };
}
const tableButton = () =>
  screen.queryByRole("button", { name: "Items in this result" });

test("an unavailable answer with a fallback shows the notice and the fallback rows", async () => {
  const rows = vi.fn().mockResolvedValue(fallbackRows());
  const { controller } = setup(
    [unavailable(1, { modelRef: "fallback", resultRevision: revision })],
    rows,
  );
  await act(async () => controller.render(request(1)));
  expect(
    screen.getByRole("heading", { name: "The space can't be attributed" }),
  ).toBeTruthy();
  expect(rows).toHaveBeenCalledWith({
    modelRef: "fallback",
    resultRevision: revision,
    start: 0,
    count: 100,
  });
  fireEvent.click(tableButton()!);
  const table = screen.getByRole("table", {
    name: visualizationMessage("en", "loadedGraphRows"),
  });
  expect(within(table).getByText(firstName())).toBeTruthy();
});

test("an unavailable answer without a fallback shows the notice only", async () => {
  const rows = vi.fn();
  const { controller } = setup([unavailable(1, null)], rows);
  await act(async () => controller.render(request(1)));
  expect(
    screen.getByRole("heading", { name: "The space can't be attributed" }),
  ).toBeTruthy();
  expect(tableButton()).toBeNull();
  expect(rows).not.toHaveBeenCalled();
});

test("a fallback pinned to another result revision is ignored", async () => {
  const rows = vi.fn();
  const { controller } = setup(
    [unavailable(1, { modelRef: "fallback", resultRevision: "another" })],
    rows,
  );
  await act(async () => controller.render(request(1)));
  expect(tableButton()).toBeNull();
  expect(rows).not.toHaveBeenCalled();
  expect(controller.getSnapshot().fallbackPage).toBeNull();
});

test("the next model restores the chart, drops the fallback table and keeps the earlier graph meanwhile", async () => {
  const rows = vi.fn().mockResolvedValue(fallbackRows());
  const { controller, container } = setup(
    [
      model(1),
      unavailable(2, { modelRef: "fallback", resultRevision: revision }),
      model(3),
    ],
    rows,
  );
  await act(async () => controller.render(request(1)));
  await act(async () => controller.render(request(2)));
  expect(container.querySelector("svg.du-geometry")).toBeNull();
  expect(tableButton()).not.toBeNull();
  expect(controller.getSnapshot().graph?.frame.model.modelRef).toBe("model-1");
  await act(async () => controller.render(request(3)));
  expect(container.querySelector("svg.du-geometry")).not.toBeNull();
  expect(tableButton()).toBeNull();
  expect(controller.getSnapshot().fallbackPage).toBeNull();
});

test("a page that arrives after the answer changed is dropped", async () => {
  let finish!: (rows: VizRows) => void;
  const rows = vi.fn(
    () => new Promise<VizRows>((resolve) => (finish = resolve)),
  );
  const { controller } = setup(
    [
      unavailable(1, { modelRef: "fallback", resultRevision: revision }),
      model(2),
    ],
    rows,
  );
  await act(async () => controller.render(request(1)));
  await act(async () => controller.render(request(2)));
  await act(async () => finish(fallbackRows()));
  expect(controller.getSnapshot().fallbackPage).toBeNull();
  expect(controller.getSnapshot().pendingRows).toBeNull();
});

test("rows of another model or revision are refused while a fallback is shown", async () => {
  const rows = vi.fn().mockResolvedValue(fallbackRows());
  const { controller } = setup(
    [unavailable(1, { modelRef: "fallback", resultRevision: revision })],
    rows,
  );
  await act(async () => controller.render(request(1)));
  await expect(
    controller.rows({
      modelRef: "elsewhere",
      resultRevision: revision,
      start: 0,
      count: 10,
    }),
  ).rejects.toThrow();
});

test("a failed read of the fallback rows offers a retry that shows them", async () => {
  const rows = vi
    .fn()
    .mockRejectedValueOnce(new Error("busy"))
    .mockResolvedValueOnce(fallbackRows());
  const { controller } = setup(
    [unavailable(1, { modelRef: "fallback", resultRevision: revision })],
    rows,
  );
  await act(async () => controller.render(request(1)));
  expect(screen.getByText("Rows could not be loaded.")).toBeTruthy();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Retry rows" }));
  });
  expect(rows).toHaveBeenCalledTimes(2);
  expect(screen.queryByText("Rows could not be loaded.")).toBeNull();
  expect(tableButton()).not.toBeNull();
});

test("a page that does not match its request is reported as a failed read", async () => {
  const rows = vi
    .fn()
    .mockResolvedValue({ ...fallbackRows(), resultRevision: "another" });
  const { controller } = setup(
    [unavailable(1, { modelRef: "fallback", resultRevision: revision })],
    rows,
  );
  await act(async () => controller.render(request(1)));
  expect(controller.getSnapshot().fallbackPage).toBeNull();
  expect(screen.getByText("Rows could not be loaded.")).toBeTruthy();
});
