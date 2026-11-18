import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { EmptyResult } from "../src/features/visualization/EmptyResult";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";

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
const chart = (root: ParentNode) => root.querySelector("svg.du-geometry");

function presenter(
  edit: (input: ReturnType<typeof modelFixture>) => void,
  language: Language = "en",
) {
  const input = modelFixture();
  edit(input);
  return render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={textFor(language)}
    />,
  );
}

test.each([
  [
    "en",
    "There are no items to draw in this scope",
    "Scope total: 0 items",
    "Items not drawn: 12 items",
  ],
  [
    "ko",
    "이 범위에는 그릴 항목이 없습니다",
    "범위 합계: 0개",
    "그리지 않은 항목: 12개",
  ],
] as const)(
  "a model with no shapes says so with its total and unavailable count (%s)",
  (language, title, total, unknown) => {
    const { container } = presenter((input) => {
      input.frame.model.shapes = [];
      input.page.response.rows = [];
      input.page.response.totalCount = 0;
      input.frame.model.totals.scope.quantity = {
        state: "exact",
        unit: "items",
        value: "0",
      };
      input.frame.model.summary.unknownItems = {
        state: "exact",
        unit: "items",
        value: "12",
      };
    }, language);
    expect(chart(container)).toBeNull();
    expect(screen.getByText(title)).toBeTruthy();
    expect(screen.getByText(total)).toBeTruthy();
    expect(screen.getByText(unknown)).toBeTruthy();
    expect(
      screen.queryByText(visualizationMessage(language, "rowsNotRetrieved")),
    ).toBeNull();
    expect(
      screen.getByRole("button", {
        name: visualizationMessage(language, "measurementBasisTotals"),
      }),
    ).toBeTruthy();
  },
);

test("an empty model with no unavailable items shows no count", () => {
  presenter((input) => {
    input.frame.model.shapes = [];
    input.frame.model.summary.unknownItems = {
      state: "exact",
      unit: "items",
      value: "0",
    };
  });
  expect(screen.queryByText(/^Items not drawn: /)).toBeNull();
});

test("the empty result alone does not depend on loaded rows", () => {
  const input = modelFixture();
  input.frame.model.shapes = [];
  render(<EmptyResult frame={input.frame} text={textFor("en")} />);
  expect(
    screen.getByText("There are no items to draw in this scope"),
  ).toBeTruthy();
});
