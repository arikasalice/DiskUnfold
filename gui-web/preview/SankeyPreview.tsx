import { useEffect } from "react";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import type {
  SizeValue,
  VizLegendEntry,
  VizQuantity,
  VizRelationship,
  VizRow,
  VizShape,
} from "../src/ipc";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import { modelFixture } from "./model-fixture";

/**
 * The Sankey (F06 README 12.1, #296): one scope root, three folders and seven
 * types, among them 「확장자 없음」 and 「유형 확인 불가」, laid out as the engine
 * lays them out. Scene "focus-type" focuses the 「유형 확인 불가」 node. Synthetic
 * values; no product command reads them.
 */
const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) =>
        message.replace("{" + name + "}", countText(name, value)),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);

/**
 * A count parameter in digit groups ("100,663,296"), as the product's locale
 * bundle writes a parameter typed count; any other value as it is.
 */
function countText(name: string, value: string | bigint) {
  const digits = String(value);
  return name === "count" && /^[0-9]+$/.test(digits)
    ? BigInt(digits).toLocaleString("ko-KR")
    : digits;
}
const MiB = 1024n * 1024n;
const quantity = (mib: number): VizQuantity => ({
  state: "exact",
  unit: "bytes",
  value: String(BigInt(mib) * MiB),
});
const size = (mib: number): SizeValue => ({
  state: "exact",
  bytes: String(BigInt(mib) * MiB),
  basis: "logical",
  evidence: "measured",
});
const TYPES: [string, string][] = [
  ["type.document", "ui.visualization.legend.type.document"],
  ["type.image", "ui.visualization.legend.type.image"],
  ["type.video", "ui.visualization.legend.type.video"],
  ["type.archive", "ui.visualization.legend.type.archive"],
  ["type.unclassified", "ui.visualization.legend.type.unclassified"],
  ["type.no_extension", "ui.visualization.legend.type.no_extension"],
  ["unclassified", "ui.visualization.legend.type.unavailable"],
];
/** Each folder's bytes by type, in type order. */
const FOLDERS: [string, number[]][] = [
  ["Media", [0, 20, 96, 0, 0, 12, 0]],
  ["Projects", [24, 0, 0, 0, 40, 6, 0]],
  ["Misc", [0, 0, 0, 16, 8, 0, 6]],
];
const GAP = 20_000;
const LIMIT = 1_000_000;
const sum = (values: number[]) => values.reduce((a, b) => a + b, 0);

/** Node tops and heights of one column, in order, with a gap between nodes. */
function stack(values: number[]) {
  const shown = values.filter((value) => value > 0).length;
  const scale = (LIMIT - GAP * Math.max(0, shown - 1)) / sum(values);
  let top = 0;
  return values.map((value) => {
    const at = { top, height: Math.round(value * scale) };
    if (value > 0) top += at.height + GAP;
    return at;
  });
}
const rect = (left: number, top: number, height: number) => ({
  primitive: "rect",
  points: [
    { x: left, y: top },
    { x: left + 12_000, y: top + height },
  ],
});
const polygon = (
  x1: number,
  y1: number,
  h1: number,
  x2: number,
  y2: number,
  h2: number,
) => ({
  primitive: "polygon",
  points: [
    { x: x1, y: y1 },
    { x: x2, y: y2 },
    { x: x2, y: y2 + h2 },
    { x: x1, y: y1 + h1 },
  ],
});

function fixture() {
  const { frame, page } = modelFixture();
  const { model } = frame;
  const base = model.shapes[0]!;
  const baseRow = page.response.rows[0]!;
  const folderTotals = FOLDERS.map(([, values]) => sum(values));
  const typeTotals = TYPES.map((_, index) =>
    sum(FOLDERS.map(([, values]) => values[index]!)),
  );
  const total = sum(folderTotals);
  frame.completeness = "complete";
  page.response.completeness = "complete";
  model.representation = {
    ...model.representation,
    format: "sankey",
    colorCriterion: "type",
    colorMode: "classification",
  };
  model.denominator = {
    ...model.denominator,
    basis: "scope",
    value: quantity(total),
    size: size(total),
  };
  model.totals.scope = { quantity: quantity(total), sourceSize: size(total) };
  model.totals.drawn = model.totals.scope;
  const ratio = (mib: number) => (mib / total).toFixed(4);
  const folders = stack(folderTotals);
  const types = stack(typeTotals);
  const item = (itemNo: number) => ({ ...base.itemRef!, itemNo });
  const shapes: VizShape[] = [
    {
      ...base,
      itemRef: item(0),
      shapeRef: "sankey.scope",
      size: size(total),
      legendClassKey: "type.folder",
      itemKind: "folder",
      column: "scope",
      geometry: rect(0, 0, LIMIT),
    },
  ];
  const rows: VizRow[] = [
    {
      ...baseRow,
      itemRef: item(0),
      displayName: "C:\\Users\\민서",
      size: size(total),
      ratio: "1",
      denominator: model.denominator,
      legendClassKey: "type.folder",
      itemKind: "folder",
    },
  ];
  const relationships: VizRelationship[] = [];
  // Where the next band leaves each folder and enters each type.
  const typeIn = types.map(({ top }) => top);
  FOLDERS.forEach(([name, values], index) => {
    const at = folders[index]!;
    shapes.push({
      ...base,
      itemRef: item(index + 1),
      shapeRef: "sankey.folder." + index,
      size: size(folderTotals[index]!),
      legendClassKey: "type.folder",
      itemKind: "folder",
      column: "folder",
      geometry: rect(494_000, at.top, at.height),
    });
    relationships.push({
      kind: "viz.sankey.link",
      fromShapeRef: "sankey.scope",
      toShapeRef: "sankey.folder." + index,
      quantity: quantity(folderTotals[index]!),
      geometry: polygon(12_000, at.top, at.height, 494_000, at.top, at.height),
    });
    let out = at.top;
    values.forEach((mib, type) => {
      if (mib === 0) return;
      const from = Math.round((mib / folderTotals[index]!) * at.height);
      const into = Math.round((mib / typeTotals[type]!) * types[type]!.height);
      relationships.push({
        kind: "viz.sankey.link",
        fromShapeRef: "sankey.folder." + index,
        toShapeRef: "sankey.type." + type,
        quantity: quantity(mib),
        geometry: polygon(506_000, out, from, 988_000, typeIn[type]!, into),
      });
      out += from;
      typeIn[type]! += into;
    });
    rows.push({
      ...baseRow,
      itemRef: item(index + 1),
      displayName: name,
      size: size(folderTotals[index]!),
      ratio: ratio(folderTotals[index]!),
      denominator: model.denominator,
      legendClassKey: "type.folder",
      itemKind: "folder",
      links: values.flatMap((mib, type) =>
        mib
          ? [
              {
                toRowRef: "group:viz.sankey.type." + TYPES[type]![0],
                quantity: quantity(mib),
              },
            ]
          : [],
      ),
    });
  });
  TYPES.forEach(([key, labelKey], index) => {
    const at = types[index]!;
    const groupRef = "viz.sankey.type." + key;
    shapes.push({
      ...base,
      itemRef: null,
      groupRef,
      matchKey: { state: "not_applicable" },
      rootBranchRef: null,
      shapeRef: "sankey.type." + index,
      size: size(typeTotals[index]!),
      legendClassKey: key,
      column: "type",
      geometry: rect(988_000, at.top, at.height),
    });
    rows.push({
      ...baseRow,
      itemRef: null,
      groupRef,
      displayName: "",
      labelKey,
      size: size(typeTotals[index]!),
      ratio: ratio(typeTotals[index]!),
      denominator: model.denominator,
      legendClassKey: key,
      displayFlags: [],
    });
  });
  model.shapes = shapes;
  model.relationships = relationships;
  model.legend = TYPES.map(([key, labelKey], index): VizLegendEntry => ({
    key,
    labelKey,
    itemCount: { state: "exact", unit: "items", value: "1" },
    total: {
      quantity: quantity(typeTotals[index]!),
      sourceSize: size(typeTotals[index]!),
    },
    condition: null,
  }));
  page.response.rows = rows;
  page.response.totalCount = rows.length;
  return { frame, page };
}

export function SankeyPreview({ scene }: { scene: string }) {
  const { frame, page } = fixture();
  useEffect(() => {
    if (scene !== "focus-type") return;
    const node = Array.from(
      document.querySelectorAll<HTMLElement>(".du-geometry-target"),
    ).find((option) =>
      option.getAttribute("aria-label")?.startsWith("유형 확인 불가"),
    );
    node?.focus();
  }, [scene]);
  const width = Math.max(320, window.innerWidth - 48);
  const beside = width >= 720 + 256;
  return (
    <main className="du-sankey-preview" style={{ padding: 24 }}>
      <VisualizationModelPresenter
        frame={frame}
        page={page}
        view={{
          viewport: {
            width: beside ? width - 256 : width,
            height: 420,
            scale: 1,
          },
          visibleWindow: { left: 0, top: 0, right: LIMIT, bottom: LIMIT },
        }}
        selected={null}
        onSelect={() => {}}
        legendSide={beside ? "right" : "below"}
        text={text}
      />
    </main>
  );
}
