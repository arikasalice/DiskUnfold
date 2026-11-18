import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { ControlledVisualization } from "../src/features/visualization/ControlledVisualization";
import { VisualizationController } from "../src/features/visualization/controller";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizRenderRequest, VizRenderResponse } from "../src/ipc";

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
const model = (seq: number): VizRenderResponse => {
  const f = modelFixture().frame;
  f.requestSeq = seq;
  f.model.modelRef = `model-${seq}`;
  return f;
};
const working = (seq: number): VizRenderResponse => ({
  kind: "job",
  requestSeq: seq,
  resultRevision: revision,
  observedAt: null,
  completeness: "partial",
  jobRef: "job",
});
const UPDATED = "Latest result applied";

function setup(answers: VizRenderResponse[], onApplyRevision?: () => void) {
  const renderPort = vi.fn();
  for (const answer of answers) renderPort.mockResolvedValueOnce(answer);
  const controller = new VisualizationController({
    render: renderPort,
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const shown = render(
    <ControlledVisualization
      controller={controller}
      text={text}
      {...(onApplyRevision ? { onApplyRevision } : {})}
    />,
  );
  return { controller, ...shown };
}
/** Every live region says its text inside a visually hidden box. */
const announced = () =>
  screen.getAllByRole("status").filter((node) => node.textContent === UPDATED);

test("a first opening is not announced", async () => {
  const { controller, container } = setup([model(1)]);
  await act(async () => controller.render(request(1)));
  expect(announced()).toHaveLength(0);
  expect(container.textContent).not.toContain(UPDATED);
});

test("a chart that grows with a running scan is not announced", async () => {
  const { controller } = setup([model(1), model(2), model(3)]);
  await act(async () => controller.render(request(1)));
  await act(async () => controller.render(request(2)));
  await act(async () => controller.render(request(3)));
  expect(announced()).toHaveLength(0);
});

test("a result that was computed while the reader saw working is announced once, out of sight", async () => {
  const { controller } = setup([working(1), model(2), model(3)]);
  await act(async () => controller.render(request(1)));
  await act(async () => controller.render(request(2)));
  const [status] = announced();
  expect(status).toBeTruthy();
  expect(status!.closest(".du-visually-hidden")).not.toBeNull();
  // The next model is an ordinary update: nothing more is said.
  await act(async () => controller.render(request(3)));
  expect(announced()).toHaveLength(0);
});

test("a newer result the reader asked for is announced", async () => {
  const apply = vi.fn();
  const { controller } = setup([model(1), model(2)], apply);
  await act(async () => controller.render(request(1)));
  act(() => controller.offerRevision("next"));
  fireEvent.click(screen.getByRole("button", { name: "Review newer result" }));
  expect(apply).toHaveBeenCalledWith("next");
  await act(async () => controller.render(request(2)));
  expect(announced()).toHaveLength(1);
});
