import { useState } from "react";
import { SegmentedChoice, Text } from "../src/components";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import type { VizLegendEntry, VizQuantity, VizRowsRequest } from "../src/ipc";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import { modelFixture } from "./model-fixture";

/**
 * The space treemap of F06 README 51 (#349): two folders drawn, two whose area
 * cannot be attributed, the summary line of the items not drawn, and a legend
 * with long counts. Synthetic values; no product command reads them.
 */
const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      // Counts read as the app's locale layer writes them, with digit groups.
      (message, [name, value]) =>
        message.replace(
          "{" + name + "}",
          name === "count" && /^\d+$/.test(String(value))
            ? Number(value).toLocaleString("ko-KR")
            : String(value),
        ),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);
const GiB = 1024n ** 3n;
const bytes = (gib: number) =>
  String((BigInt(Math.round(gib * 10)) * GiB) / 10n);
const lower = (value: string) => ({
  state: "lower_bound" as const,
  bytes: value,
  basis: "allocated" as const,
  evidence: "os_reported" as const,
});
const lowerQuantity = (value: string): VizQuantity => ({
  state: "lower_bound",
  unit: "bytes",
  value,
});
const items = (value: string): VizQuantity => ({
  state: "exact",
  unit: "items",
  value,
});
const legend: [string, string, number][] = [
  ["type.document", "88174", 6],
  ["type.audio", "7370", 1.9],
  ["type.image", "294030", 209.7],
  ["type.installer_image", "511", 340.5],
  ["type.video", "7291", 439.2],
  ["type.archive", "15442", 96.4],
];

function fixture() {
  const { frame, page } = modelFixture();
  const { model } = frame;
  const names = ["문서", "음악", "사진 보관함", "공유 자료"];
  const sizes = [bytes(120), bytes(40), bytes(640), bytes(300)];
  const scope = bytes(1100);
  frame.completeness = "missing";
  page.response.completeness = "missing";
  model.representation.sizeBasis = "allocated";
  model.representation.colorCriterion = "type";
  model.representation.colorMode = "classification";
  model.totals.scope = {
    quantity: lowerQuantity(scope),
    sourceSize: lower(scope),
  };
  // F03 says an allocated total may count shared space twice (#351).
  model.totals.scopeMeasurementLimits = [
    "space.allocated_not_collected",
    "space.allocated_may_include_shared",
  ];
  model.totals.drawn = {
    quantity: lowerQuantity(bytes(160)),
    sourceSize: lower(bytes(160)),
  };
  model.denominator = {
    ...model.denominator,
    value: lowerQuantity(scope),
    size: lower(scope),
  };
  // The drawn folders divide the whole chart (README 51.2).
  model.shapes = model.shapes.slice(0, 2).map((shape, index) => ({
    ...shape,
    size: lower(sizes[index]!),
    legendClassKey: index === 0 ? "type.document" : "type.audio",
    geometry: {
      primitive: "rect",
      points:
        index === 0
          ? [
              { x: 0, y: 0 },
              { x: 746000, y: 1000000 },
            ]
          : [
              { x: 750000, y: 0 },
              { x: 1000000, y: 1000000 },
            ],
    },
  }));
  model.legend = legend.map(([key, count, gib]): VizLegendEntry => ({
    key,
    labelKey: "ui.visualization.legend." + key,
    itemCount: items(count),
    total: {
      quantity: lowerQuantity(bytes(gib)),
      sourceSize: lower(bytes(gib)),
    },
    condition: null,
  }));
  model.summary = {
    ...model.summary,
    unknownItems: items("2"),
    unknownSize: lowerQuantity(bytes(940)),
    firstUnknownRow: 2,
  };
  page.response.rows = page.response.rows.map((row, index) => ({
    ...row,
    displayName: names[index]!,
    size: lower(sizes[index]!),
    ratio: null,
    denominator: model.denominator,
    legendClassKey:
      index < 2 ? model.shapes[index]!.legendClassKey : "unclassified",
    displayFlags: index < 2 ? [] : ["viz.flag.unknown"],
  }));
  return { frame, page };
}

/**
 * The same scope once #364 draws a partly measured folder (README 51.1): 「사진 보관함」
 * is drawn by its measured bytes as a lower bound, and only 「공유 자료」 is not drawn.
 */
function partialFixture() {
  const { frame, page } = fixture();
  const { model } = frame;
  const rows = [2, 0, 1, 3].map((index) => page.response.rows[index]!);
  const keys = ["type.image", "type.document", "type.audio"];
  const rects = [
    [0, 0, 796000, 1000000],
    [800000, 0, 1000000, 746000],
    [800000, 750000, 1000000, 1000000],
  ];
  const base = model.shapes[0]!;
  model.shapes = rects.map(([left, top, right, bottom], index) => ({
    ...base,
    itemRef: rows[index]!.itemRef,
    rootBranchRef: rows[index]!.itemRef,
    shapeRef: "fixture-partial-" + index,
    size: rows[index]!.size,
    legendClassKey: keys[index]!,
    geometry: {
      primitive: "rect",
      points: [
        { x: left!, y: top! },
        { x: right!, y: bottom! },
      ],
    },
  }));
  page.response.rows = rows.map((row, index) => ({
    ...row,
    legendClassKey: keys[index] ?? "unclassified",
    displayFlags: index < 3 ? [] : ["viz.flag.unknown"],
  }));
  model.totals.drawn = {
    quantity: lowerQuantity(bytes(800)),
    sourceSize: lower(bytes(800)),
  };
  model.summary = {
    ...model.summary,
    unknownItems: items("1"),
    unknownSize: lowerQuantity(bytes(300)),
    firstUnknownRow: 3,
  };
  return { frame, page };
}

export function SpaceUndrawnPreview({ scene }: { scene: string }) {
  const [{ frame, page }] = useState(() =>
    scene === "partial" ? partialFixture() : fixture(),
  );
  const [shown, setShown] = useState(page);
  const [criterion, setCriterion] = useState("type");
  const width = Math.max(320, window.innerWidth - 48);
  const beside = width >= 720 + 256;
  return (
    <main className="du-space-undrawn-preview" style={{ padding: 24 }}>
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
