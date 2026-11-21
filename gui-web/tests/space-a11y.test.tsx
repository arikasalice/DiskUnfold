import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { ChartLegend } from "../src/components/charts/chart-legend";
import { GeometryRenderer } from "../src/components/charts/geometry-renderer";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import { shapeLabels } from "../src/viz/model-text";
import type { VizText } from "../src/viz";

afterEach(cleanup);

const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);

test("#201: a folder drawn closed, such as a bar, is named a folder", () => {
  const { frame, page } = modelFixture();
  const shape = { ...frame.model.shapes[0]!, itemKind: "folder" as const };
  const row = { ...page.response.rows[0]!, displayName: "docs" };
  const top = shapeLabels(frame.model, shape, row, text).accessibleLabel;
  expect(top).toMatch(/^docs, 폴더, /);
  expect(top).not.toContain("안에");
  const inside = shapeLabels(frame.model, shape, row, text, {
    inFolder: true,
    parentRow: { ...row, displayName: "work" },
  }).accessibleLabel;
  expect(inside).toMatch(/^docs, 폴더, work 안, /);
});

test("#201: in high contrast the legend swatches show the patterns the chart draws", () => {
  const { container } = render(
    <ChartLegend
      label="Legend"
      criterion={null}
      patternsLabel="Use patterns"
      patterns={false}
      patternsForced="Patterns are always on in high contrast"
      onPatterns={() => {}}
      entries={[
        {
          key: "type.video",
          label: "Videos",
          count: "2",
          total: "10 MiB",
          tone: "5",
          pattern: "rings",
          patternAlways: false,
        },
      ]}
      selectedKey={null}
      onSelect={() => {}}
      moreLabel={() => "more"}
      lessLabel="fewer"
    />,
  );
  expect(container.querySelector(".du-chart-legend__pattern")).not.toBeNull();
});

test("#201: a 24 px bar takes its pattern; the size rule counts the shape before its gap", () => {
  const { container } = render(
    <GeometryRenderer
      modelRef="model-1"
      resultRevision="revision-1"
      label="Chart"
      selectedText="selected"
      fallbackNotice="Graph unavailable"
      missingFallbackNotice="Table unavailable"
      width={600}
      height={24}
      viewport={{ left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 }}
      selectedRowRef={null}
      onSelect={() => {}}
      fallback={{
        modelRef: "model-1",
        resultRevision: "revision-1",
        content: null,
      }}
      relations={[]}
      patterns
      shapes={[
        {
          shapeRef: "bar",
          rowRef: "row-bar",
          accessibleLabel: "clip.mp4, 6 MiB",
          tone: "5",
          pattern: "rings",
          patternAlways: false,
          geometry: {
            primitive: "rect",
            points: [
              { x: 0, y: 0 },
              { x: 1_000_000, y: 1_000_000 },
            ],
          },
        },
      ]}
    />,
  );
  expect(container.querySelector(".du-geometry-pattern")).not.toBeNull();
});
