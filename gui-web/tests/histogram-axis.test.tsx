import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { useHistogramAxis } from "../src/app/histogram-axis";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import type { VizCatalog } from "../src/ipc";
import {
  createProductionVizText,
  shapeLabels,
  visualizationMessage,
} from "../src/viz";
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
const catalog = (measures: string[]): VizCatalog => ({
  formats: ["treemap", "bar", "histogram"],
  strategies: [{ id: "viz.histogram", revision: 1 }],
  measures,
  colorCriteria: ["none"],
  denominatorCriteria: ["scope"],
  interpretationTableRevision: 1,
  requiredDataTableRevision: 1,
  legendRuleRevision: 1,
});

function Probe({ format, measures }: { format: string; measures: string[] }) {
  const axis = useHistogramAxis(format, catalog(measures), undefined, text);
  return (
    <>
      <output aria-label="measure">{axis.measure}</output>
      {axis.control}
    </>
  );
}
const measure = () => screen.getByLabelText("measure").textContent;

test("F06 README 43: the histogram starts on the item count, keeps the chosen axis and asks for size elsewhere", () => {
  const { rerender } = render(
    <Probe format="histogram" measures={["size", "count"]} />,
  );
  screen.getByRole("radiogroup", { name: say("axisChoice") });
  const count = screen.getByRole<HTMLInputElement>("radio", {
    name: say("measureCount"),
  });
  expect(count.checked).toBe(true);
  expect(measure()).toBe("count");

  fireEvent.click(screen.getByRole("radio", { name: say("measureSize") }));
  expect(measure()).toBe("size");

  // Another format measures size and has no axis to choose.
  rerender(<Probe format="bar" measures={["size", "count"]} />);
  expect(measure()).toBe("size");
  expect(screen.queryByRole("radiogroup")).toBeNull();
  // Back on the histogram the chosen axis is still size.
  rerender(<Probe format="histogram" measures={["size", "count"]} />);
  expect(
    screen.getByRole<HTMLInputElement>("radio", { name: say("measureSize") })
      .checked,
  ).toBe(true);
  expect(measure()).toBe("size");
});

test("F06 README 43: without the count measure in the catalog the histogram offers no axis choice", () => {
  render(<Probe format="histogram" measures={["size"]} />);
  expect(screen.queryByRole("radiogroup")).toBeNull();
  expect(measure()).toBe("size");
});

function histogram(measureKey: "count" | "size") {
  const input = modelFixture();
  const model = input.frame.model;
  model.representation = {
    ...model.representation,
    format: "histogram",
    measure: measureKey,
  };
  const shape = model.shapes[0]!;
  shape.itemRef = null;
  shape.groupRef = "viz.group.bin.3";
  shape.memberCount = { unit: "items", state: "exact", value: "6" };
  if (measureKey === "count") shape.quantity = shape.memberCount;
  return { input, shape };
}

test.each(["count", "size"] as const)(
  "F06 README 43: an interval names its item count and its size on the %s axis",
  (axis) => {
    const { input, shape } = histogram(axis);
    const { accessibleLabel } = shapeLabels(
      input.frame.model,
      shape,
      undefined,
      text,
    );
    expect(accessibleLabel).toContain(`항목 6개, ${text.size(shape.size)}`);
  },
);

test.each([
  ["count", "measureCount"],
  ["size", "axisTitleSize"],
] as const)(
  "F06 README 43: the histogram's axis title follows the %s measure, beside the status line control",
  (axis, title) => {
    const { input } = histogram(axis);
    const { container } = render(
      <VisualizationModelPresenter
        frame={input.frame}
        page={input.page}
        view={{
          viewport: { width: 960, height: 330, scale: 1 },
          visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
        }}
        selected={null}
        onSelect={() => {}}
        statusAction={<button type="button">axis</button>}
        text={text}
      />,
    );
    expect(container.querySelector(".du-chart-axis-title")?.textContent).toBe(
      say(title),
    );
    expect(screen.getByRole("button", { name: "axis" })).toBeTruthy();
    // The rows of a histogram are size ranges, and the line above its table counts ranges.
    expect(
      screen.getByText(
        say("rangesOnePage").replace(
          "{count}",
          String(input.page.response.totalCount),
        ),
      ),
    ).toBeTruthy();
  },
);
