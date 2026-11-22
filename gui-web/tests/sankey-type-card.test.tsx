import { expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import {
  createProductionVizText,
  shapeLabels,
  visualizationMessage,
} from "../src/viz";
import type { VizQuantity, VizRow, VizShape } from "../src/ipc";

// #296, design 2026-11-23: a Sankey type node's card names the folders that
// send to it, the five largest first and the rest summed, and does not repeat
// its own class.

const text = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);
const MiB = 1024n * 1024n;
const bytes = (mib: number): VizQuantity => ({
  unit: "bytes",
  state: "exact",
  value: String(BigInt(mib) * MiB),
});

test("a type node lists its incoming folders, largest first, five and the rest", () => {
  const { frame } = modelFixture();
  const model = frame.model;
  model.representation = {
    ...model.representation,
    format: "sankey",
    colorCriterion: "type",
  };
  model.legend = [
    {
      ...model.legend[0]!,
      key: "type.video",
      labelKey: "ui.visualization.legend.type.video",
    },
  ];
  const base = frame.model.shapes[0]!;
  const shape: VizShape = {
    ...base,
    itemRef: null,
    groupRef: "viz.sankey.type.type.video",
    legendClassKey: "type.video",
    shapeRef: "sankey.type.4",
    column: "type",
  };
  const row: VizRow = {
    ...modelFixture().page.response.rows[0]!,
    itemRef: null,
    groupRef: "viz.sankey.type.type.video",
    displayName: "",
    labelKey: "ui.visualization.legend.type.video",
    legendClassKey: "type.video",
  };
  const folder = (name: string): VizRow => ({
    ...modelFixture().page.response.rows[0]!,
    displayName: name,
  });
  const incoming = [3, 90, 1, 40, 7, 12, 2].map((mib, index) => ({
    row: folder(`F${index}`),
    quantity: bytes(mib),
  }));
  const labels = shapeLabels(model, shape, row, text, { incoming });
  const lines = labels.card!.lines;
  expect(labels.card!.title).toBe("동영상");
  expect(lines).toContain("이어진 항목");
  const from = lines.filter((line) => line.includes("에서, "));
  expect(from.map((line) => line.split("에서")[0])).toEqual([
    "F1",
    "F3",
    "F5",
    "F4",
    "F0",
  ]);
  expect(lines[lines.length - 1]).toBe("나머지 폴더 2개 · 3.0 MiB");
  // The class is the node's own name: no line or spoken part repeats it.
  expect(lines.filter((line) => line === "동영상")).toHaveLength(0);
  expect(
    labels.accessibleLabel.split(", ").filter((part) => part === "동영상"),
  ).toHaveLength(1);
});
