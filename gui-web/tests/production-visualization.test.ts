import { describe, expect, test } from "vitest";
import { supportsBrowseVisualization } from "../src/app/browse-visualization";
import type { BrowseVisualizationSelection } from "../src/app/browse-visualization";
import { productionVisualizationProvider } from "../src/app/production-visualization";
import type { SizeBasis, VizCatalog } from "../src/ipc";

const location = { resultVersion: "result-7", itemNo: 12 };
function selection(
  sizeBasis: SizeBasis = "logical",
): BrowseVisualizationSelection {
  return {
    resultVersion: "result-7",
    location,
    display: { system: "iec", unit: "auto" },
    sizeBasis,
  };
}
const catalog: VizCatalog = {
  formats: ["treemap", "bar"],
  strategies: [{ id: "viz.treemap", revision: 3 }],
  measures: ["size"],
  colorCriteria: ["none"],
  denominatorCriteria: ["scope"],
  interpretationTableRevision: 1,
  requiredDataTableRevision: 1,
  legendRuleRevision: 1,
};

describe("F06 FR1.HD1.app-host.DD1: the product provider", () => {
  test("pins the request to the selected result and folder", () => {
    for (const basis of ["logical", "allocated", "unique_physical"] as const) {
      const assembly = productionVisualizationProvider(
        selection(basis),
        catalog,
      );
      expect(assembly?.request).toMatchObject({
        resultRevision: "result-7",
        format: "treemap",
        aggregateScopeRef: location,
        navigationScopeRef: location,
        measure: "size",
        sizeBasis: basis,
        denominatorBasis: "scope",
        depthLimit: 2,
      });
    }
  });

  test("F06 FR2.HD1.gui-web.DD1: asks for no color and no classification, and names no strategy", () => {
    const assembly = productionVisualizationProvider(selection(), catalog);
    expect(assembly?.request.colorCriterion).toBe("none");
    expect(assembly?.request.classificationCriterion).toBe("none");
    expect(assembly?.request).not.toHaveProperty("strategy");
    expect(assembly?.request).not.toHaveProperty("focusItemRef");
    expect(assembly?.request.visibleWindow).toEqual({
      left: 0,
      top: 0,
      right: 1_000_000,
      bottom: 1_000_000,
    });
  });

  test("leaves text to the view, loads the first rows and passes the Browse gate", () => {
    const assembly = productionVisualizationProvider(selection(), catalog);
    expect(assembly).not.toBeNull();
    expect(assembly?.text).toBeUndefined();
    expect(assembly?.autoLoadRows).toBe(true);
    if (assembly)
      expect(supportsBrowseVisualization(assembly, catalog, selection())).toBe(
        true,
      );
  });

  test("offers nothing when the catalog does not serve the request", () => {
    const without = (patch: Partial<VizCatalog>) =>
      productionVisualizationProvider(selection(), { ...catalog, ...patch });
    expect(without({ formats: ["bar"] })).toBeNull();
    expect(without({ measures: ["growth"] })).toBeNull();
    expect(without({ colorCriteria: ["type"] })).toBeNull();
    expect(without({ colorCriteria: [] })).toBeNull();
    expect(without({ denominatorCriteria: ["parent_folder"] })).toBeNull();
    expect(without({})).not.toBeNull();
  });

  test("offers nothing for a folder of another result version", () => {
    expect(
      productionVisualizationProvider(
        { ...selection(), location: { resultVersion: "result-6", itemNo: 12 } },
        catalog,
      ),
    ).toBeNull();
  });
});
