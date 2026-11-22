import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { ControlledVisualization } from "../src/features/visualization/ControlledVisualization";
import { VisualizationController } from "../src/features/visualization/controller";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizRenderRequest, VizRenderResponse } from "../src/ipc";

afterEach(cleanup);

// #365: the lines above the chart wrap (at 700 or 1130 CSS px) and take more
// than the room kept for them, so the chart is asked again at another height.
// The drawn chart stays without a loading or computing notice over it, and the
// new model replaces it quietly. Any other change still says it is working.

const text = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("en", key),
    ),
  { system: "iec", unit: "auto" },
);
const WORKING = visualizationMessage("en", "working");
const LOADING = visualizationMessage("en", "loading");
const UPDATED = visualizationMessage("en", "graphUpdated");
const revision = modelFixture().frame.resultRevision;
const request = (
  seq: number,
  size: { width: number; height: number },
): VizRenderRequest => ({
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
  viewport: { ...size, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
});
const model = (seq: number): VizRenderResponse => {
  const frame = modelFixture().frame;
  frame.requestSeq = seq;
  frame.model.modelRef = `model-${seq}`;
  return frame;
};
const working = (seq: number): VizRenderResponse => ({
  kind: "job",
  requestSeq: seq,
  resultRevision: revision,
  observedAt: null,
  completeness: "partial",
  jobRef: "job",
});

function setup() {
  const renderPort = vi.fn();
  const controller = new VisualizationController({
    render: renderPort,
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  const shown = render(
    <ControlledVisualization controller={controller} text={text} />,
  );
  return { controller, renderPort, ...shown };
}
const said = (container: HTMLElement, words: string) =>
  (container.textContent ?? "").includes(words);

test.each([700, 1130])(
  "at %s px a taller status area refits the drawn chart without a notice over it",
  async (width) => {
    const { controller, renderPort, container } = setup();
    renderPort.mockResolvedValueOnce(model(1));
    await act(async () =>
      controller.render(request(1, { width, height: 500 })),
    );
    expect(screen.getByRole("listbox")).toBeTruthy();

    // While the request is in flight, and then while the engine computes it.
    let answer: (value: VizRenderResponse) => void = () => {};
    renderPort.mockReturnValueOnce(
      new Promise<VizRenderResponse>((resolve) => (answer = resolve)),
    );
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = controller.render(request(2, { width, height: 470 }));
    });
    expect(controller.getSnapshot().phase).toBe("loading");
    expect(said(container, LOADING)).toBe(false);
    await act(async () => {
      answer(working(2));
      await pending;
    });
    expect(controller.getSnapshot().phase).toBe("job");
    expect(said(container, WORKING)).toBe(false);
    expect(screen.getByRole("listbox")).toBeTruthy();

    renderPort.mockResolvedValueOnce(model(3));
    await act(async () =>
      controller.render(request(3, { width, height: 470 })),
    );
    expect(controller.getSnapshot().graph?.frame.model.modelRef).toBe(
      "model-3",
    );
    expect(said(container, UPDATED)).toBe(false);
  },
);

test("another width still says the chart is being calculated", async () => {
  const { controller, renderPort, container } = setup();
  renderPort.mockResolvedValueOnce(model(1)).mockResolvedValueOnce(working(2));
  await act(async () =>
    controller.render(request(1, { width: 1130, height: 500 })),
  );
  await act(async () =>
    controller.render(request(2, { width: 700, height: 500 })),
  );
  expect(said(container, WORKING)).toBe(true);
  expect(screen.getByRole("listbox")).toBeTruthy();
});
