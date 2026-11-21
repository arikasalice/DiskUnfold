import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";

afterEach(cleanup);

const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);
const say = (key: Parameters<typeof visualizationMessage>[1]) =>
  visualizationMessage("ko", key);

function shown(limits?: string[]) {
  const input = modelFixture();
  const { model } = input.frame;
  model.representation.sizeBasis = "allocated";
  if (limits) model.totals.scopeMeasurementLimits = limits;
  return render(
    <VisualizationModelPresenter
      frame={input.frame}
      page={input.page}
      view={{
        viewport: { width: 960, height: 330, scale: 1 },
        visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
      }}
      selected={null}
      onSelect={() => {}}
      text={text}
    />,
  );
}

test("#351: an allocated total that may count shared space says it may exceed the actual usage", () => {
  const { container } = shown([
    "space.aux_not_included",
    "space.allocated_may_include_shared",
  ]);
  const line = container.querySelector(".du-scope-total-line");
  expect(line?.textContent).toContain(say("scopeMayExceedUsage"));
  expect(line?.textContent).toContain(" · " + say("scopeMayExceedUsage"));
  // The full sentence is the tooltip and is read by assistive technology too.
  expect(line?.getAttribute("title")).toBe(say("scopeMayExceedUsageTooltip"));
  expect(screen.getByText(say("scopeMayExceedUsageTooltip"))).toBeTruthy();
});

test("#351: without that key, or with only keys the screen does not know, the line says nothing more", () => {
  for (const limits of [undefined, ["space.later_key"]]) {
    const { container } = shown(limits);
    expect(container.querySelector(".du-scope-total-line")).toBeNull();
    expect(container.textContent).not.toContain(say("scopeMayExceedUsage"));
    expect(container.textContent).not.toContain("space.later_key");
    cleanup();
  }
});
