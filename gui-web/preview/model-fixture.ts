import type { SizeValue, VizQuantity, VizShape } from "../src/ipc";
import { SIZE_UNIT_RULES } from "../src/ipc";
import { formatByteUnit } from "../src/i18n/bytes";
import type {
  ModelResponse,
  ModelRowsPage,
} from "../src/features/visualization/model-adapter";
import type { VizText } from "../src/features/visualization/model-text";
import { fixtures } from "./surface-fixtures";

const revision = "018f0000-0000-7000-8000-000000000001";
const item = (itemNo: number) => ({ resultVersion: revision, itemNo });
const quantity = (value: string): VizQuantity => ({
  state: "exact",
  value,
  unit: "bytes",
});
const size = (bytes: string): SizeValue => ({
  state: "exact",
  bytes,
  basis: "logical",
  evidence: "measured",
});
const total = (bytes: string) => ({
  quantity: quantity(bytes),
  sourceSize: size(bytes),
});
const zero = { state: "exact", value: "0", unit: "items" } as const;
const values = ["51539607552", "25769803776", "17179869184", "8589934592"];
const rectangles = [
  [
    { x: 0, y: 0 },
    { x: 500000, y: 1000000 },
  ],
  [
    { x: 504000, y: 0 },
    { x: 1000000, y: 500000 },
  ],
  [
    { x: 504000, y: 510000 },
    { x: 834000, y: 1000000 },
  ],
  [
    { x: 838000, y: 510000 },
    { x: 1000000, y: 1000000 },
  ],
];

/** Generated wire types, synthetic values. No product command reads this fixture. */
export function modelFixture(): { frame: ModelResponse; page: ModelRowsPage } {
  const denominator = {
    basis: "scope",
    value: quantity("103079215104"),
    size: size("103079215104"),
  } as const;
  const shapes: VizShape[] = values.map((value, index) => ({
    itemRef: item(index + 1),
    groupRef: null,
    matchKey: { state: "unknown", reasonKey: "fixture.identity_unavailable" },
    geometry: { primitive: "rect", points: rectangles[index]! },
    size: size(value),
    legendClassKey: "fixture.personal",
    displayFlags: [],
    shapeRef: `fixture-shape-${index}`,
    parentShapeRef: null,
    visibleDepth: 1,
    rootBranchRef: item(index + 1),
  }));
  const frame: ModelResponse = {
    kind: "model",
    requestSeq: 1,
    resultRevision: revision,
    observedAt: null,
    completeness: "partial",
    model: {
      modelRef: "synthetic-layout",
      conditionFingerprint: "synthetic-condition",
      aggregateScopeRef: item(0),
      navigationScopeRef: item(0),
      focusItemRef: null,
      origin: "current_analysis",
      staleness: { state: "unknown", reasonKey: "fixture.not_live" },
      representation: {
        format: "treemap",
        measure: "size",
        sizeBasis: "logical",
        classificationCriterion: "type",
        colorCriterion: "type",
        denominatorBasis: "scope",
        colorMode: "monochrome",
        labelMode: "name_size",
      },
      denominator,
      totals: {
        scope: total("103079215104"),
        drawn: total("103079215104"),
        grouped: total("0"),
        filteredOut: total("0"),
      },
      shapes,
      relationships: [],
      legend: [
        {
          key: "fixture.personal",
          labelKey: "fixture.personal",
          itemCount: { ...zero, value: "4" },
          total: total("103079215104"),
          condition: null,
        },
      ],
      summary: {
        groupedItems: zero,
        omittedItems: zero,
        unknownItems: zero,
        reasonKeys: [],
      },
      strategy: { id: "viz.treemap", revision: 1 },
      tableRevisions: {
        interpretation: 1,
        requiredData: 1,
        legend: 1,
        viewPolicy: 1,
        sizeRule: { id: "size.logical", revision: 1 },
      },
    },
  };
  const page: ModelRowsPage = {
    request: {
      modelRef: frame.model.modelRef,
      resultRevision: revision,
      start: 0,
      count: 20,
    },
    response: {
      resultRevision: revision,
      observedAt: null,
      completeness: "partial",
      totalCount: 4,
      rows: shapes.map((shape, index) => ({
        itemRef: shape.itemRef,
        groupRef: null,
        displayName: fixtures.workspace.rows[index]![0],
        size: shape.size,
        ratio: [
          "0.5",
          "0.25",
          "0.16666666666666666667",
          "0.08333333333333333333",
        ][index]!,
        denominator,
        legendClassKey: shape.legendClassKey,
        displayFlags: [],
      })),
    },
  };
  return { frame, page };
}

const labels: Record<string, string> = {
  current_analysis: "Current analysis",
  partial: "Partial result",
  complete: "Complete result",
  current: "Current",
  stale: "Older observation",
  unknown: "Freshness unknown",
  scope: "Current scope",
  treemap: "Treemap",
  size: "Size",
  logical: "Logical size",
  "fixture.personal": "Personal data",
  "fixture.not_live": "Synthetic preview · no live observation",
};

export const fixtureText: VizText = {
  key: (key) => labels[key] ?? "Additional result limitation",
  quantity: (value) =>
    value.state === "unknown"
      ? "Unavailable"
      : `${value.state === "lower_bound" ? "At least " : ""}${value.value} ${value.unit}`,
  size: (value) =>
    value.state === "unknown"
      ? "Unavailable"
      : `${value.state === "lower_bound" ? "At least " : ""}${value.bytes} bytes · ${value.evidence}`,
  observedAt: (value) => value ?? "Observation time unavailable",
};

const displayBytes = (bytes: string, lowerBound: boolean) =>
  `${lowerBound ? "At least " : ""}${formatByteUnit(BigInt(bytes), SIZE_UNIT_RULES.iec, SIZE_UNIT_RULES.decimalPlaces, "auto", lowerBound)}`;

export const fixtureDisplayText: VizText = {
  ...fixtureText,
  size: (value) =>
    value.state === "unknown"
      ? "Unavailable"
      : displayBytes(value.bytes, value.state === "lower_bound"),
  quantity: (value) =>
    value.state !== "unknown" &&
    value.unit === "bytes" &&
    /^\d+$/.test(value.value)
      ? displayBytes(value.value, value.state === "lower_bound")
      : fixtureText.quantity(value),
};
