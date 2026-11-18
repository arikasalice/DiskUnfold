import { expect, test } from "vitest";
import { supportsBrowseVisualization } from "../src/app/browse-visualization";
import type {
  BrowseVisualizationAssembly,
  BrowseVisualizationSelection,
} from "../src/app/browse-visualization";
import type { VizCatalog } from "../src/ipc";
import type { VisualizationViewRequest } from "../src/viz";
import { fixtureText } from "../preview/model-fixture";

const location = { resultVersion: "result", itemNo: 7 };
const selection: BrowseVisualizationSelection = {
  resultVersion: "result",
  location,
  display: { system: "iec", unit: "auto" },
  sizeBasis: "logical",
};
const catalog: VizCatalog = {
  formats: ["treemap"],
  strategies: [{ id: "viz.treemap", revision: 2 }],
  measures: ["size"],
  colorCriteria: ["none"],
  denominatorCriteria: ["scope"],
  interpretationTableRevision: 1,
  requiredDataTableRevision: 1,
  legendRuleRevision: 1,
};

function assembly(
  strategy?: VisualizationViewRequest["strategy"],
): BrowseVisualizationAssembly {
  const request: VisualizationViewRequest = {
    resultRevision: "result",
    format: "treemap",
    aggregateScopeRef: location,
    navigationScopeRef: location,
    measure: "size",
    sizeBasis: "logical",
    classificationCriterion: "none",
    colorCriterion: "none",
    denominatorBasis: "scope",
    viewport: { width: 1, height: 1, scale: 1 },
    visibleWindow: { left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 },
    minAreaPx2: 16,
    depthLimit: 1,
    ...(strategy ? { strategy } : {}),
  };
  return {
    request,
    canDisplay: () => true,
    text: fixtureText,
    autoLoadRows: true,
  };
}

test("F06 FR2.HD1.gui-web.DD1: a request without a strategy passes the gate", () => {
  expect(supportsBrowseVisualization(assembly(), catalog, selection)).toBe(
    true,
  );
});

test("F06 FR2.HD1.gui-web.DD1: a named strategy must be listed by the catalog", () => {
  expect(
    supportsBrowseVisualization(
      assembly({ id: "viz.treemap", revision: 2 }),
      catalog,
      selection,
    ),
  ).toBe(true);
  expect(
    supportsBrowseVisualization(
      assembly({ id: "viz.treemap", revision: 1 }),
      catalog,
      selection,
    ),
  ).toBe(false);
  expect(
    supportsBrowseVisualization(
      assembly({ id: "viz.unlisted", revision: 2 }),
      catalog,
      selection,
    ),
  ).toBe(false);
});

test("F06 FR2.HD1.gui-web.DD1: omitting the strategy does not relax the other checks", () => {
  const other = assembly();
  expect(
    supportsBrowseVisualization(
      { ...other, request: { ...other.request, format: "bar" } },
      catalog,
      selection,
    ),
  ).toBe(false);
  expect(
    supportsBrowseVisualization(
      { ...other, request: { ...other.request, resultRevision: "older" } },
      catalog,
      selection,
    ),
  ).toBe(false);
  expect(
    supportsBrowseVisualization(
      { ...other, request: { ...other.request, colorCriterion: "type" } },
      catalog,
      selection,
    ),
  ).toBe(false);
});
