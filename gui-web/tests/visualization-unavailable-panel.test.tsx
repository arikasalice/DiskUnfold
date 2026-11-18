import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { UnavailablePanel } from "../src/features/visualization/UnavailablePanel";
import { ControlledVisualization } from "../src/features/visualization/ControlledVisualization";
import { VisualizationController } from "../src/features/visualization/controller";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import type { VizRenderRequest, VizRenderResponse } from "../src/ipc";

afterEach(cleanup);

type Language = "en" | "ko";
const textFor = (language: Language): VizText =>
  createProductionVizText(
    (key, values) =>
      Object.entries(values ?? {}).reduce(
        (message, [name, value]) => message.replace(`{${name}}`, String(value)),
        visualizationMessage(language, key),
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
function frame(seq: number) {
  const f = modelFixture().frame;
  f.requestSeq = seq;
  f.model.modelRef = `model-${seq}`;
  return f;
}
const unavailable = (
  seq: number,
  reasonKey = "viz.unavailable.no_baseline",
): VizRenderResponse => ({
  kind: "unavailable",
  requestSeq: seq,
  resultRevision: revision,
  observedAt: null,
  completeness: "partial",
  reasonKey,
  availableFormats: ["viz.bar"],
  fallback: null,
});
function controllerFor(...responses: VizRenderResponse[]) {
  const renderPort = vi.fn();
  for (const response of responses) renderPort.mockResolvedValueOnce(response);
  return new VisualizationController({
    render: renderPort,
    rows: vi.fn(),
    cancel: vi.fn(),
    canDisplay: () => true,
  });
}
const chart = (root: ParentNode) => root.querySelector("svg.du-geometry");

test("an unavailable answer replaces the chart, keeps the model, and the next model restores it", async () => {
  const controller = controllerFor(frame(1), unavailable(2), frame(3));
  const { container } = render(
    <>
      <button>Outside</button>
      <ControlledVisualization controller={controller} text={textFor("en")} />
    </>,
  );
  await act(async () => controller.render(request(1)));
  expect(chart(container)).not.toBeNull();
  const outside = screen.getByRole("button", { name: "Outside" });
  outside.focus();

  await act(async () => controller.render(request(2)));
  const title = "There is no earlier result to compare";
  expect(screen.getByRole("heading", { name: title })).toBeTruthy();
  expect(chart(container)).toBeNull();
  expect(controller.getSnapshot().graph?.frame.model.modelRef).toBe("model-1");
  expect(document.activeElement).toBe(outside);
  // The sentence is drawn once, inside the status region that announces it, and
  // nothing lists formats.
  expect(screen.getAllByText(title)).toHaveLength(1);
  expect(
    screen
      .getAllByRole("status")
      .filter((node) => node.textContent?.includes(title)),
  ).toHaveLength(1);
  expect(container.textContent).not.toContain("Available formats");

  await act(async () => controller.render(request(3)));
  expect(screen.queryByRole("heading", { name: title })).toBeNull();
  expect(chart(container)).not.toBeNull();
  expect(controller.getSnapshot().graph?.frame.model.modelRef).toBe("model-3");
});

test("the unavailable panel offers the way back to the folders beside its text", async () => {
  const controller = controllerFor(unavailable(1));
  const back = vi.fn();
  render(
    <ControlledVisualization
      controller={controller}
      text={textFor("en")}
      backAction={{ label: "Back to folders", onPress: back }}
    />,
  );
  await act(async () => controller.render(request(1)));
  fireEvent.click(screen.getByRole("button", { name: "Back to folders" }));
  expect(back).toHaveBeenCalledTimes(1);
  // The sentence is drawn once, not in a status line and again in a box.
  expect(
    screen.getAllByText(
      "This chart compares two points in time. Analyze the same scope again or choose a saved result.",
    ),
  ).toHaveLength(1);
});

test("a first answer that is unavailable shows only the panel", async () => {
  const controller = controllerFor(
    unavailable(1, "viz.unavailable.negative_flow"),
  );
  const { container } = render(
    <ControlledVisualization controller={controller} text={textFor("en")} />,
  );
  await act(async () => controller.render(request(1)));
  expect(
    screen.getByRole("heading", { name: "This chart can't show decreases" }),
  ).toBeTruthy();
  expect(chart(container)).toBeNull();
  expect(controller.getSnapshot().graph).toBeNull();
  expect(
    screen.queryByText("Rows have not been retrieved for this graph."),
  ).toBeNull();
});

const wording = {
  "viz.unavailable.no_baseline": [
    [
      "There is no earlier result to compare",
      "This chart compares two points in time. Analyze the same scope again or choose a saved result.",
    ],
    [
      "비교할 이전 결과가 없습니다",
      "이 그래프는 두 시점의 결과를 비교합니다. 같은 범위를 한 번 더 분석하거나 보관한 결과를 고르면 볼 수 있습니다.",
    ],
  ],
  "viz.unavailable.single_observation": [
    [
      "There is only one observation",
      "A trend line needs two or more observations. Changes appear from the next analysis.",
    ],
    [
      "관측이 한 번뿐입니다",
      "추세선은 관측이 두 번 이상 있어야 그릴 수 있습니다. 다음 분석부터 변화가 그려집니다.",
    ],
  ],
  "viz.unavailable.no_attribution": [
    [
      "The space can't be attributed",
      "The same files belong to more than one folder, or where they belong couldn't be confirmed. The list still shows the original values.",
    ],
    [
      "공간이 어디에 속하는지 나눌 수 없습니다",
      "같은 파일이 여러 폴더에 걸쳐 있거나 속한 곳을 확인할 수 없습니다. 목록에서는 원래 수치를 볼 수 있습니다.",
    ],
  ],
  "viz.unavailable.no_item_sizes": [
    [
      "No item sizes are available",
      "No item in this scope has a confirmed size yet. If a scan is running, items appear shortly.",
    ],
    [
      "크기를 확인한 항목이 없습니다",
      "이 범위에서 크기를 확인한 항목이 아직 없습니다. 스캔 중이면 잠시 뒤 나타납니다.",
    ],
  ],
  "viz.unavailable.negative_flow": [
    [
      "This chart can't show decreases",
      "Sankey shows only amounts of zero or more. Use the waterfall chart to see increases and decreases.",
    ],
    [
      "줄어든 양은 이 그래프로 그릴 수 없습니다",
      "생키는 0 이상의 양만 그립니다. 늘고 준 양은 워터폴에서 볼 수 있습니다.",
    ],
  ],
  // The engine does not produce this key, so it gets the general wording.
  "viz.unavailable.no_stable_order": [
    [
      "This chart isn't available",
      "The reason couldn't be confirmed. The list shows the same result.",
    ],
    [
      "이 그래프를 그릴 수 없습니다",
      "사유를 확인할 수 없습니다. 목록에서는 같은 결과를 볼 수 있습니다.",
    ],
  ],
  "viz.unavailable.from_the_future": [
    [
      "This chart isn't available",
      "The reason couldn't be confirmed. The list shows the same result.",
    ],
    [
      "이 그래프를 그릴 수 없습니다",
      "사유를 확인할 수 없습니다. 목록에서는 같은 결과를 볼 수 있습니다.",
    ],
  ],
} as const;

test.each(
  Object.entries(wording).flatMap(([reason, pairs]) =>
    (["en", "ko"] as const).map(
      (language, index) => [reason, language, pairs[index]!] as const,
    ),
  ),
)("the panel words %s in %s", (reason, language, [title, body]) => {
  render(<UnavailablePanel reasonKey={reason} text={textFor(language)} />);
  expect(screen.getByRole("heading", { name: title })).toBeTruthy();
  expect(screen.getByText(body)).toBeTruthy();
  expect(document.body.textContent).not.toContain("viz.unavailable");
});
