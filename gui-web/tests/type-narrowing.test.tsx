import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { useLegendChoice } from "../src/app/legend-choice";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import type { TypeNarrowing } from "../src/features/visualization/ModelLegend";
import type { QueryCondition, VizCatalog } from "../src/ipc";
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

const video: QueryCondition = {
  kind: "predicate",
  predicate: {
    kind: "registered",
    id: "pred.file_type",
    revision: 1,
    operand: "dHlwZS52aWRlbw==",
  },
};

const bytes = (value: string) =>
  ({
    quantity: { unit: "bytes", state: "exact", value },
    sourceSize: null,
  }) as const;

/** A treemap coloured by type whose video row narrows; 300 bytes were left out. */
function typed(shapes = true) {
  const input = modelFixture();
  const model = input.frame.model;
  model.representation = { ...model.representation, colorCriterion: "type" };
  model.totals = { ...model.totals, filteredOut: bytes("300") };
  if (!shapes) model.shapes = [];
  model.legend = [
    {
      key: "type.video",
      labelKey: "ui.visualization.legend.type.video",
      itemCount: { unit: "items", state: "exact", value: "2" },
      total: bytes("100"),
      condition: video,
    },
    {
      key: "unclassified",
      labelKey: "ui.visualization.legend.type.unavailable",
      itemCount: { unit: "items", state: "exact", value: "1" },
      total: bytes("5"),
      condition: null,
    },
  ];
  return input;
}

function narrowing(over: Partial<TypeNarrowing> = {}): TypeNarrowing {
  return {
    key: null,
    label: null,
    inactive: false,
    announced: null,
    choose: vi.fn(),
    ...over,
  };
}

function presenter(
  input: ReturnType<typeof modelFixture>,
  chosen: TypeNarrowing,
) {
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
      narrowing={chosen}
      text={text}
    />,
  );
}

const legendOption = (name: string) =>
  within(screen.getByRole("listbox", { name: say("legendTitle") }))
    .getAllByRole("option")
    .find((option) => option.textContent?.startsWith(name))!;

test("F06 README 46.1: pressing a legend row narrows to its type with the row's condition", () => {
  const chosen = narrowing();
  presenter(typed(), chosen);
  fireEvent.click(legendOption(say("legend.type.video")));
  expect(chosen.choose).toHaveBeenCalledWith({
    key: "type.video",
    label: say("legend.type.video"),
    condition: video,
  });
  // A row without a condition does not narrow.
  fireEvent.click(legendOption(say("legend.type.unavailable")));
  expect(chosen.choose).toHaveBeenCalledTimes(1);
});

test("F06 README 46.2, 46.7: a narrowed treemap names the type, its total against the scope and what was left out", () => {
  const chosen = narrowing({
    key: "type.video",
    label: say("legend.type.video"),
  });
  presenter(typed(), chosen);
  const clear = screen.getByRole("button", { name: say("filterClearType") });
  expect(clear.textContent).toContain(say("legend.type.video"));
  expect(
    legendOption(say("legend.type.video")).getAttribute("aria-selected"),
  ).toBe("true");
  expect(screen.getByText(/· 범위 합계/).textContent).toMatch(
    /^동영상 100 bytes · 범위 합계 96\.0 GiB$/,
  );
  expect(screen.getByText("필터 제외 300 bytes")).toBeTruthy();
  fireEvent.click(clear);
  expect(chosen.choose).toHaveBeenCalledWith(null);
  // The same row again lets go too.
  fireEvent.click(legendOption(say("legend.type.video")));
  expect(chosen.choose).toHaveBeenLastCalledWith(null);
});

test("F06 README 46.5: a format that does not narrow keeps the chip, inactive, and says so", () => {
  presenter(
    typed(),
    narrowing({
      key: "type.video",
      label: say("legend.type.video"),
      inactive: true,
    }),
  );
  expect(screen.getByText(say("filterNotApplicable"))).toBeTruthy();
  expect(screen.queryByText(/· 범위 합계/)).toBeNull();
});

test("F06 README 46.1: a folder without the type says so in the chart's place and offers to clear", () => {
  const chosen = narrowing({
    key: "type.video",
    label: say("legend.type.video"),
  });
  presenter(typed(false), chosen);
  expect(
    screen.getByText("이 폴더에는 「동영상」에 해당하는 파일이 없습니다"),
  ).toBeTruthy();
  const buttons = screen.getAllByRole("button", {
    name: say("filterClearType"),
  });
  fireEvent.click(buttons[buttons.length - 1]!);
  expect(chosen.choose).toHaveBeenCalledWith(null);
});

const catalog: VizCatalog = {
  formats: ["treemap", "bar", "histogram"],
  strategies: [{ id: "viz.treemap", revision: 1 }],
  measures: ["size"],
  colorCriteria: ["none", "type"],
  denominatorCriteria: ["scope"],
  interpretationTableRevision: 1,
  requiredDataTableRevision: 1,
  legendRuleRevision: 1,
};
const owner = { request: { colorCriterion: "none" } };

test("F06 README 46.5, 46.6: the treemap sends the chosen type, other formats keep it inactive, and a change is announced", () => {
  let format = "treemap";
  const hook = renderHook(() => useLegendChoice(format, catalog, owner, text));
  expect(hook.result.current.request).toEqual({ colorCriterion: "type" });
  act(() =>
    hook.result.current.narrowing?.choose({
      key: "type.video",
      label: "동영상",
      condition: video,
    }),
  );
  expect(hook.result.current.request).toEqual({
    colorCriterion: "type",
    condition: { kind: "normalized", condition: video },
  });
  expect(hook.result.current.narrowing?.announced).toEqual({ type: "동영상" });
  for (const other of ["bar", "histogram"]) {
    format = other;
    hook.rerender();
    expect(hook.result.current.request).not.toHaveProperty("condition");
    expect(hook.result.current.narrowing?.inactive).toBe(true);
  }
  format = "treemap";
  hook.rerender();
  expect(hook.result.current.request).toHaveProperty("condition");
  act(() => hook.result.current.narrowing?.choose(null));
  expect(hook.result.current.request).toEqual({ colorCriterion: "type" });
  expect(hook.result.current.narrowing?.announced).toBe("all");
});
