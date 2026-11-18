import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";

afterEach(cleanup);
const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
};

function show(staleness: {
  state: "current" | "stale" | "unknown";
  reasonKey: string | null;
}) {
  const input = modelFixture();
  input.frame.model.staleness = staleness;
  render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={fixtureText}
    />,
  );
}

test("F06 FR9.HD1.gui-web.DD1: a result whose freshness is not watched has no freshness badge", () => {
  show({ state: "unknown", reasonKey: "viz.staleness.unwatched" });
  expect(screen.queryByText("Freshness unknown")).toBeNull();
  expect(screen.queryByText("Current")).toBeNull();
  expect(screen.queryByText("Older observation")).toBeNull();
});

test("F06 FR9.HD1.gui-web.DD1: a current result has no freshness badge", () => {
  show({ state: "current", reasonKey: null });
  expect(screen.queryByText("Current")).toBeNull();
  expect(screen.queryByText("Freshness unknown")).toBeNull();
  expect(screen.queryByText("Older observation")).toBeNull();
});

test("F06 FR9.HD1.gui-web.DD1: a result that changed after the analysis has a freshness badge", () => {
  show({ state: "stale", reasonKey: null });
  expect(screen.getByText("Older observation")).toBeDefined();
  expect(screen.queryByText("Freshness unknown")).toBeNull();
});
