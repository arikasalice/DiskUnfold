import { useState } from "react";
import { SegmentedChoice, Text } from "../src/components";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import type {
  SizeValue,
  VizLegendEntry,
  VizQuantity,
  VizRow,
  VizRowsRequest,
  VizShape,
  VizTypeSlice,
} from "../src/ipc";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import { modelFixture } from "./model-fixture";

/**
 * Folders divided by type (F06 README 54, #298): three folders, one only a lower
 * bound and one with six types, two files and the Other group, as a treemap and
 * as bars. Synthetic values; no product command reads them.
 */
const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) =>
        message.replace("{" + name + "}", String(value)),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);
const GiB = 1024n ** 3n;
const bytes = (gib: number) => String(BigInt(gib) * GiB);
const size = (gib: number, lower = false): SizeValue => ({
  state: lower ? "lower_bound" : "exact",
  bytes: bytes(gib),
  basis: "logical",
  evidence: "measured",
});
const quantity = (gib: number, lower = false): VizQuantity => ({
  state: lower ? "lower_bound" : "exact",
  unit: "bytes",
  value: bytes(gib),
});
const items = (value: number): VizQuantity => ({
  state: "exact",
  unit: "items",
  value: String(value),
});
const slice = (key: string, gib: number, lower = false): VizTypeSlice => ({
  classKey: key,
  itemCount: {
    ...items(Math.max(1, Math.round(gib / 3))),
    ...(lower ? { state: "lower_bound" } : {}),
  } as VizQuantity,
  total: quantity(gib, lower),
});

type Entry = {
  name: string;
  gib: number;
  lower?: boolean;
  key: string;
  folder?: boolean;
  slices?: VizTypeSlice[];
};
const ENTRIES: Entry[] = [
  {
    name: "작업",
    gib: 400,
    key: "type.folder",
    folder: true,
    slices: [
      slice("type.video", 250),
      slice("type.document", 100),
      slice("type.image", 50),
    ],
  },
  {
    name: "사진 보관함",
    gib: 300,
    lower: true,
    key: "type.folder",
    folder: true,
    slices: [slice("type.image", 200, true), slice("type.video", 60, true)],
  },
  {
    name: "백업",
    gib: 150,
    key: "type.folder",
    folder: true,
    slices: [
      slice("type.archive", 90),
      slice("type.installer_image", 40),
      slice("type.document", 10),
      slice("type.audio", 5),
      slice("type.no_extension", 3),
      slice("type.unclassified", 2),
    ],
  },
  { name: "발표 영상.mp4", gib: 100, key: "type.video" },
  { name: "보고서.pdf", gib: 50, key: "type.document" },
];
const LEGEND: [string, number, number][] = [
  ["type.document", 160, 3],
  ["type.audio", 5, 1],
  ["type.image", 250, 2],
  ["type.installer_image", 40, 1],
  ["type.video", 410, 3],
  ["type.archive", 90, 1],
  ["type.unclassified", 2, 1],
  ["type.no_extension", 3, 1],
];
const TREEMAP: [number, number, number, number][] = [
  [0, 0, 446000, 1000000],
  [450000, 0, 1000000, 596000],
  [450000, 600000, 746000, 1000000],
  [750000, 600000, 1000000, 836000],
  [750000, 840000, 1000000, 1000000],
];

function fixture(format: "treemap" | "bar") {
  const { frame, page } = modelFixture();
  const { model } = frame;
  const base = model.shapes[0]!;
  const baseRow = page.response.rows[0]!;
  frame.completeness = "complete";
  page.response.completeness = "complete";
  model.representation = {
    ...model.representation,
    format,
    colorCriterion: "type",
    colorMode: "classification",
  };
  const scope = {
    quantity: quantity(1000, true),
    sourceSize: size(1000, true),
  };
  model.totals.scope = scope;
  model.totals.drawn = scope;
  model.denominator = {
    ...model.denominator,
    value: scope.quantity,
    size: scope.sourceSize,
  };
  const bar = (index: number) => {
    const top = 40000 + index * 160000;
    const right = Math.round(((ENTRIES[index]?.gib ?? 30) / 400) * 1000000);
    return [0, top, right, top + 60000] as const;
  };
  model.shapes = ENTRIES.map((entry, index): VizShape => {
    const [left, top, right, bottom] =
      format === "bar" ? bar(index) : TREEMAP[index]!;
    return {
      ...base,
      itemRef: { ...base.itemRef!, itemNo: index + 1 },
      rootBranchRef: { ...base.itemRef!, itemNo: index + 1 },
      shapeRef: "type-slices-" + index,
      size: size(entry.gib, entry.lower),
      legendClassKey: entry.key,
      itemKind: entry.folder ? "folder" : "file",
      ...(entry.slices ? { typeSlices: entry.slices } : {}),
      geometry: {
        primitive: "rect",
        points: [
          { x: left, y: top },
          { x: right, y: bottom },
        ],
      },
    };
  });
  const rows: VizRow[] = ENTRIES.map((entry, index) => ({
    ...baseRow,
    itemRef: { ...baseRow.itemRef!, itemNo: index + 1 },
    displayName: entry.name,
    size: size(entry.gib, entry.lower),
    ratio: null,
    denominator: model.denominator,
    legendClassKey: entry.key,
    displayFlags: [],
    itemKind: entry.folder ? "folder" : "file",
    ...(entry.slices ? { typeSlices: entry.slices } : {}),
  }));
  if (format === "bar") {
    const [left, top, right, bottom] = bar(5);
    model.shapes.push({
      ...base,
      itemRef: null,
      groupRef: "viz.group.other",
      matchKey: { state: "not_applicable" },
      shapeRef: "type-slices-other",
      rootBranchRef: null,
      size: size(30),
      legendClassKey: "unclassified",
      memberCount: items(3),
      geometry: {
        primitive: "rect",
        points: [
          { x: left, y: top },
          { x: right, y: bottom },
        ],
      },
    });
    rows.push({
      ...baseRow,
      itemRef: null,
      groupRef: "viz.group.other",
      displayName: "ui.visualization.group.other",
      labelKey: "ui.visualization.group.other",
      memberCount: items(3),
      size: size(30),
      ratio: null,
      denominator: model.denominator,
      legendClassKey: "unclassified",
      displayFlags: [],
    });
  }
  page.response.rows = rows;
  page.response.totalCount = rows.length;
  model.legend = LEGEND.map(([key, gib, count]): VizLegendEntry => ({
    key,
    labelKey: "ui.visualization.legend." + key,
    itemCount: items(count),
    total: { quantity: quantity(gib), sourceSize: size(gib) },
    condition: null,
  }));
  return { frame, page };
}

export function TypeSlicesPreview({ scene }: { scene: string }) {
  const [{ frame, page }] = useState(() =>
    fixture(scene.startsWith("bar") ? "bar" : "treemap"),
  );
  const [shown, setShown] = useState(page);
  const [criterion, setCriterion] = useState("type");
  const width = Math.max(320, window.innerWidth - 48);
  const beside = width >= 720 + 256;
  return (
    <main className="du-type-slices-preview" style={{ padding: 24 }}>
      <VisualizationModelPresenter
        frame={frame}
        page={shown}
        view={{
          viewport: {
            width: beside ? width - 256 : width,
            height: 420,
            scale: 1,
          },
          visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
        }}
        selected={null}
        onSelect={() => {}}
        onPage={(request: VizRowsRequest) => setShown({ ...shown, request })}
        legendSide={beside ? "right" : "below"}
        legend={{
          criterion: (
            <>
              <Text kind="muted">{text.message("colorBy")}</Text>
              <SegmentedChoice
                label={text.message("colorBy")}
                value={criterion}
                onChange={setCriterion}
                options={[
                  { value: "none", label: text.message("colorNone") },
                  { value: "type", label: text.message("colorType") },
                ]}
              />
            </>
          ),
          patterns: false,
          onPatterns: () => {},
        }}
        text={text}
      />
    </main>
  );
}
