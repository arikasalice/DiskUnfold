import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { useLegendChoice } from "../src/app/legend-choice";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import type { VizCatalog } from "../src/ipc";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import { legendLook } from "../src/viz/legend-style";
import { ChartLegend } from "../src/components/charts/chart-legend";

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

test("F06 README 4.2, 44.3: a type keeps its place everywhere and the neutral classes always show their pattern", () => {
  const order = ["type.video", "type.document", "unclassified"];
  expect(legendLook("type.document", order)).toEqual({
    tone: "1",
    pattern: null,
    patternAlways: false,
  });
  expect(legendLook("type.video", order).tone).toBe("5");
  expect(legendLook("type.installer_image", order).tone).toBe("4");
  expect(legendLook("type.unclassified", order)).toMatchObject({
    tone: "8",
    pattern: "rings",
  });
  expect(legendLook("type.no_extension", order)).toEqual({
    tone: "neutral-2",
    pattern: "sparse-dots",
    patternAlways: true,
  });
  expect(legendLook("unclassified", order)).toEqual({
    tone: "neutral",
    pattern: "dense-hatch",
    patternAlways: true,
  });
  expect(legendLook("type.folder", order).tone).toBe("folder");
  // An unknown class takes the place of its position; from the ninth on with a pattern.
  const many = Array.from({ length: 10 }, (_, index) => `fixture.${index}`);
  expect(legendLook("fixture.2", many)).toMatchObject({
    tone: "3",
    patternAlways: false,
  });
  expect(legendLook("fixture.9", many)).toMatchObject({
    tone: "2",
    patternAlways: true,
  });
});

function typed() {
  const input = modelFixture();
  const model = input.frame.model;
  model.representation = { ...model.representation, colorCriterion: "type" };
  const keys = [
    "type.document",
    "type.video",
    "type.unclassified",
    "type.folder",
  ];
  model.shapes.forEach((shape, index) => {
    shape.legendClassKey = keys[index]!;
  });
  const entry = (key: string, labelKey: string) => ({
    key,
    labelKey,
    itemCount: { unit: "items", state: "exact", value: "1" } as const,
    total: model.totals.scope,
    condition: null,
  });
  model.legend = [
    entry("type.document", "ui.visualization.legend.type.document"),
    entry("type.video", "ui.visualization.legend.type.video"),
    entry("type.unclassified", "ui.visualization.legend.type.unclassified"),
  ];
  return input;
}

function presenter(input: ReturnType<typeof modelFixture>) {
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
      legend={{
        criterion: <span>criterion</span>,
        patterns: false,
        onPatterns: () => {},
      }}
      text={text}
    />,
  );
}

test("F06 FR6.HD1.gui-web.DD1: a model coloured by type draws its classes, names them and lists them without the folder", () => {
  const { container } = presenter(typed());
  const tones = Array.from(
    container.querySelectorAll(".du-geometry-shape"),
  ).map((shape) => shape.getAttribute("data-tone"));
  expect(tones).toEqual(["1", "5", "8", "folder"]);
  const legend = screen.getByRole("listbox", { name: say("legendTitle") });
  const names = within(legend)
    .getAllByRole("option")
    .map((option) => option.textContent);
  expect(names.map((name) => name?.split(/\d/)[0])).toEqual([
    say("legend.type.document"),
    say("legend.type.video"),
    say("legend.type.unclassified"),
  ]);
  // The folder is not listed, so the note says why the classes add up to less.
  expect(screen.getByText(say("legendFolderNote"))).toBeTruthy();
  // A shape says its class with its name and value.
  const options = screen.getAllByRole("option", {
    name: new RegExp(say("legend.type.video")),
  });
  expect(options.some((option) => option.closest(".du-geometry-targets"))).toBe(
    true,
  );
});

test("F06 README 4.7: choosing a class in the legend fades the shapes of the others", () => {
  const { container } = presenter(typed());
  const legend = screen.getByRole("listbox", { name: say("legendTitle") });
  const video = within(legend).getAllByRole("option")[1]!;
  act(() => {
    fireEvent.click(video);
  });
  const dimmed = Array.from(
    container.querySelectorAll(".du-geometry-shape"),
  ).map((shape) => shape.hasAttribute("data-dimmed"));
  expect(dimmed).toEqual([true, false, true, true]);
  // The choice shows as a chip on the status line, which lets go of it.
  const chipName = `${say("colorType")}: ${say("legend.type.video")}`;
  const chip = screen.getByRole("button", {
    name: say("legendChipClear").replace("{name}", chipName),
  });
  expect(chip.textContent).toBe(`${chipName} ×`);
  act(() => {
    fireEvent.click(chip);
  });
  expect(container.querySelector(".du-geometry-shape[data-dimmed]")).toBeNull();
});

test("F06 README 44 (#220): a class total that counted nothing is unavailable, not at least zero", () => {
  const input = typed();
  input.frame.model.legend[2]!.total = {
    quantity: { unit: "bytes", state: "lower_bound", value: "0" },
    sourceSize: null,
  };
  presenter(input);
  const legend = screen.getByRole("listbox", { name: say("legendTitle") });
  const other = within(legend).getAllByRole("option")[2]!;
  expect(other.textContent).toContain(say("unavailableValue"));
  expect(other.textContent).not.toContain("0");
});

const catalog = (colorCriteria: string[]): VizCatalog => ({
  formats: ["treemap", "bar", "histogram"],
  strategies: [{ id: "viz.treemap", revision: 1 }],
  measures: ["size"],
  colorCriteria,
  denominatorCriteria: ["scope"],
  interpretationTableRevision: 1,
  requiredDataTableRevision: 1,
  legendRuleRevision: 1,
});
const owner = { request: { colorCriterion: "none" } };

test("F06 README 44.1: the treemap starts coloured by type, the histogram has no legend, and a wide area puts the legend beside", () => {
  const types = catalog(["none", "type"]);
  const treemap = renderHook(() =>
    useLegendChoice("treemap", types, owner, text),
  );
  expect(treemap.result.current.colorCriterion).toBe("type");
  expect(treemap.result.current.controls).toBeDefined();
  expect(treemap.result.current.side(1000)).toBe("right");
  expect(treemap.result.current.chartWidth(1000)).toBe(1000 - 256);
  expect(treemap.result.current.side(700)).toBe("below");
  expect(treemap.result.current.chartWidth(700)).toBe(700);
  const histogram = renderHook(() =>
    useLegendChoice("histogram", types, owner, text),
  );
  expect(histogram.result.current.colorCriterion).toBe("none");
  expect(histogram.result.current.controls).toBeUndefined();
  // A catalog without the type criterion keeps the owner's.
  const older = renderHook(() =>
    useLegendChoice("bar", catalog(["none"]), owner, text),
  );
  expect(older.result.current.colorCriterion).toBe("none");
  expect(older.result.current.controls).toBeUndefined();
});

test("F06 README 44.1: choosing None keeps the choice and its control", () => {
  const hook = renderHook(() =>
    useLegendChoice("bar", catalog(["none", "type"]), owner, text),
  );
  render(<>{hook.result.current.controls?.criterion}</>);
  fireEvent.click(screen.getByRole("radio", { name: say("colorNone") }));
  hook.rerender();
  expect(hook.result.current.colorCriterion).toBe("none");
  expect(hook.result.current.controls).toBeDefined();
});

test("F06 README 44.3: in high contrast the pattern switch shows on, cannot be changed and says why", () => {
  render(
    <ChartLegend
      label="Legend"
      criterion={null}
      patternsLabel="Use patterns"
      patterns={false}
      patternsForced="Patterns are always on in high contrast"
      onPatterns={() => {}}
      entries={[
        {
          key: "type.document",
          label: "Documents",
          count: "1",
          total: "1 B",
          tone: "1",
          pattern: null,
          patternAlways: false,
        },
      ]}
      selectedKey={null}
      onSelect={() => {}}
      moreLabel={() => "more"}
      lessLabel="fewer"
    />,
  );
  const button = screen.getByRole("button", { name: "Use patterns" });
  expect(button.getAttribute("aria-pressed")).toBe("true");
  expect(
    button.hasAttribute("disabled") ||
      button.getAttribute("aria-disabled") === "true",
  ).toBe(true);
  expect(button.getAttribute("aria-describedby")).toBeTruthy();
  expect(
    document.getElementById(button.getAttribute("aria-describedby")!)
      ?.textContent,
  ).toBe("Patterns are always on in high contrast");
});
