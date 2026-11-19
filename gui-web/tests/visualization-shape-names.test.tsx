import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { ControlledVisualization } from "../src/features/visualization/ControlledVisualization";
import { VisualizationController } from "../src/features/visualization/controller";
import type { VizRenderRequest, VizRows, VizRowsRequest } from "../src/ipc";
import type { ModelResponse } from "../src/viz";

afterEach(cleanup);

const COUNT = 1200;
const revision = "018f0000-0000-7000-8000-000000000001";

/** A grid of rectangles, one item per cell, plus one shape that has no row. */
function frame(): ModelResponse {
  const value = modelFixture().frame;
  const template = value.model.shapes[0]!;
  const columns = 40;
  const shapes = Array.from({ length: COUNT + 1 }, (_, index) => {
    const x = (index % columns) * 25_000;
    const y = Math.floor(index / columns) * 30_000;
    const itemNo = index === COUNT ? 99_999 : index + 1;
    return {
      ...template,
      itemRef: { resultVersion: revision, itemNo },
      rootBranchRef: { resultVersion: revision, itemNo },
      shapeRef: `cell-${itemNo}`,
      geometry: {
        primitive: "rect",
        points: [
          { x: x + 1000, y: y + 1000 },
          { x: x + 24_000, y: y + 29_000 },
        ],
      },
    };
  });
  value.model.shapes = shapes;
  value.model.aggregateScopeRef.resultVersion = revision;
  value.model.navigationScopeRef.resultVersion = revision;
  return value;
}

function renderRequest(value: ModelResponse): VizRenderRequest {
  return {
    viewChannel: "names-test",
    requestSeq: 1,
    resultRevision: value.resultRevision,
    format: "treemap",
    aggregateScopeRef: value.model.aggregateScopeRef,
    navigationScopeRef: value.model.navigationScopeRef,
    measure: "size",
    sizeBasis: "logical",
    classificationCriterion: "none",
    colorCriterion: "none",
    denominatorBasis: "scope",
    minAreaPx2: 4,
    depthLimit: 1,
    viewport: { width: 1000, height: 1000, scale: 1 },
    visibleWindow: { left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 },
  };
}

test("F06 FR6.HD1.gui-web.DD1: every shape takes its name from its own row, whatever the row order or page", async () => {
  const model = frame();
  const base = modelFixture().page.response.rows[0]!;
  // Rows arrive newest first, so an array position never equals a shape position.
  const allRows = Array.from({ length: COUNT }, (_, index) => {
    const itemNo = COUNT - index;
    return {
      ...base,
      itemRef: { resultVersion: revision, itemNo },
      displayName: `Entry ${itemNo}`,
    };
  });
  const rows = vi.fn(async (request: VizRowsRequest): Promise<VizRows> => ({
    rows: allRows.slice(request.start, request.start + request.count),
    totalCount: COUNT,
    resultRevision: model.resultRevision,
    observedAt: model.observedAt,
    completeness: model.completeness,
  }));
  const controller = new VisualizationController({
    render: vi.fn().mockResolvedValue(model),
    rows,
    cancel: vi.fn(),
    canDisplay: () => true,
  });
  render(
    <ControlledVisualization
      controller={controller}
      text={fixtureText}
      autoLoadRows
    />,
  );
  await act(async () => controller.render(renderRequest(model)));

  const first = await screen.findByRole("option", { name: /^Entry 1 · / });
  expect(first).toBeTruthy();
  expect(screen.getByRole("option", { name: /^Entry 1200 · / })).toBeTruthy();
  expect(screen.getByRole("option", { name: /^Entry 600 · / })).toBeTruthy();
  expect(rows.mock.calls.map(([r]) => [r.start, r.count])).toEqual([
    [0, 1000],
    [1000, 201], // one read per shape, the last page asks for the extra shape
  ]);
  expect(
    screen.getByRole("option", { name: /^Item name not yet retrieved · / }),
  ).toBeTruthy();
});
