import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { visualizationMessage, visualizationReason } from "../src/viz";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";

afterEach(cleanup);
const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
};
const unwatched = "Changes made after the analysis aren't reflected";
const generic = "The reason is unavailable";

function showEvidence(reasonKey: string | null) {
  const input = modelFixture();
  input.frame.model.staleness = { state: "unknown", reasonKey };
  render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
    />,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Measurement basis and totals" }),
  );
}

test("the unwatched freshness reason shows its wording and never the raw key", () => {
  showEvidence("viz.staleness.unwatched");
  expect(screen.getByText(unwatched)).toBeDefined();
  expect(screen.queryByText("viz.staleness.unwatched")).toBeNull();
  expect(screen.queryByText(generic)).toBeNull();
});

test("a freshness reason the table does not know shows the generic wording", () => {
  showEvidence("viz.staleness.some_future_reason");
  expect(screen.getByText(generic)).toBeDefined();
  expect(screen.queryByText("viz.staleness.some_future_reason")).toBeNull();
  expect(screen.queryByText(unwatched)).toBeNull();
});

test("a model without a freshness reason shows no freshness notice", () => {
  showEvidence(null);
  expect(screen.queryByText(unwatched)).toBeNull();
  expect(screen.queryByText(generic)).toBeNull();
});

test("the reason table words the unwatched reason in both languages and keeps the unavailable reasons", () => {
  for (const [language, expected] of [
    ["en", unwatched],
    ["ko", "분석한 뒤에 바뀐 내용은 반영되지 않습니다"],
  ] as const) {
    const message = (key: Parameters<typeof visualizationMessage>[1]) =>
      visualizationMessage(language, key);
    expect(visualizationReason("viz.staleness.unwatched", message)).toBe(
      expected,
    );
    expect(visualizationReason("viz.unavailable.no_baseline", message)).toBe(
      visualizationMessage(language, "noBaseline"),
    );
    expect(visualizationReason("unlisted", message)).toBe(
      visualizationMessage(language, "unknownReason"),
    );
  }
});
