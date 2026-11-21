import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { GeometryRenderer } from "../src/components/charts/geometry-renderer";
import { stackLayout } from "../src/components/charts/slice-stack";
import type { VizTypeSlice } from "../src/ipc";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import { folderTypes } from "../src/viz/type-slices";
import { TypeSlicesPreview } from "../preview/TypeSlicesPreview";

afterEach(cleanup);

const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);
const say = (key: Parameters<typeof visualizationMessage>[1]) =>
  visualizationMessage("ko", key);

const NAMES: Record<string, string> = {
  "type.document": "문서",
  "type.audio": "오디오",
  "type.image": "이미지",
  "type.installer_image": "설치·디스크 이미지",
  "type.video": "동영상",
  "type.archive": "압축 파일",
  "type.no_extension": "확장자 없음",
};
const ORDER = Object.keys(NAMES);

function slice(
  key: string,
  bytes: number,
  state: "exact" | "lower_bound" = "exact",
): VizTypeSlice {
  return {
    classKey: key,
    itemCount: { unit: "items", state, value: "1" },
    total: { unit: "bytes", state, value: String(bytes) },
  };
}

const sliceText = {
  message: text.message,
  quantity: text.quantity,
  className: (key: string) => NAMES[key] ?? key,
};

const exact = (bytes: number) =>
  ({
    basis: "logical",
    evidence: "measured",
    state: "exact",
    bytes: String(bytes),
  }) as const;

test("README 54.5, 54.6: the five largest types are named, the rest summed, the cell names the largest", () => {
  // Largest first, as the engine sends them.
  const slices = [
    slice("type.video", 700),
    slice("type.document", 600),
    slice("type.image", 500),
    slice("type.audio", 400),
    slice("type.archive", 300),
    slice("type.no_extension", 200),
    slice("type.installer_image", 100),
  ];
  const types = folderTypes(slices, exact(2800), ORDER, sliceText);
  expect(types?.said).toEqual([
    "동영상 700 bytes",
    "문서 600 bytes",
    "이미지 500 bytes",
    "오디오 400 bytes",
    "압축 파일 300 bytes",
    "나머지 유형 2개 · 300 bytes",
  ]);
  expect(types?.cell).toBe("동영상 외 6개");
  // The bar keeps the legend order, so a type sits in the same place on every bar.
  expect(types?.bar.map((part) => part.key)).toEqual(ORDER);
  expect(types?.total).toBe(2800);
});

test("README 54.6: one type is named alone and a counted folder without files reads —", () => {
  expect(
    folderTypes([slice("type.document", 5)], exact(5), ORDER, sliceText)?.cell,
  ).toBe("문서");
  expect(folderTypes([], exact(0), ORDER, sliceText)?.cell).toBe("—");
  // Not applicable: no slices at all.
  expect(folderTypes(undefined, exact(0), ORDER, sliceText)).toBeNull();
});

test("README 54: beyond the slices of a lower bound is not split by type, never an exact rest", () => {
  const lower = {
    basis: "logical",
    evidence: "measured",
    state: "lower_bound",
    bytes: "1000",
  } as const;
  const types = folderTypes(
    [slice("type.document", 600, "lower_bound")],
    lower,
    ORDER,
    sliceText,
  );
  expect(types?.said).toEqual([
    "문서 최소 600 bytes",
    say("typeSlicesUnsplit").replace("{size}", "400 bytes"),
  ]);
  // An exact folder with exact slices says nothing more.
  expect(
    folderTypes([slice("type.document", 600)], exact(1000), ORDER, sliceText)
      ?.said,
  ).toEqual(["문서 600 bytes"]);
});

test("README 13: parts keep a 2 px gap and none is narrower than 2 px", () => {
  const parts = stackLayout([1000, 1, 1], 104);
  expect(parts?.map((part) => part.length)).toEqual([96, 2, 2]);
  expect(parts?.map((part) => part.start)).toEqual([0, 98, 102]);
  // Too narrow to draw every part at the minimum: nothing.
  expect(stackLayout([1, 1, 1], 9)).toBeNull();
});

function folderShape(height: number) {
  return {
    shapeRef: "folder",
    rowRef: "folder-row",
    accessibleLabel: "folder",
    tone: "folder",
    slices: [
      {
        key: "type.video",
        tone: "5",
        pattern: null,
        patternAlways: false,
        value: 3,
      },
      {
        key: "type.document",
        tone: "1",
        pattern: null,
        patternAlways: false,
        value: 1,
      },
    ],
    slicesTotal: 4,
    geometry: {
      primitive: "rect",
      points: [
        { x: 0, y: 0 },
        { x: 1_000_000, y: height },
      ],
    },
  };
}

function drawn(variant: "ranked" | undefined, height: number) {
  const { container } = render(
    <GeometryRenderer
      label="chart"
      selectedText="selected"
      fallbackNotice="unavailable"
      missingFallbackNotice="no table"
      selectedRowRef={null}
      fallback={{ modelRef: "m", resultRevision: "r", content: null }}
      width={400}
      height={200}
      modelRef="m"
      resultRevision="r"
      relations={[]}
      shapes={[folderShape(height)]}
      viewport={{ left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 }}
      onSelect={() => {}}
      {...(variant ? { variant } : {})}
    />,
  );
  return Array.from(
    container.querySelectorAll(".du-slice-stack [data-key]"),
  ).map((part) => part.getAttribute("data-key"));
}

test("README 54, 7B: a closed folder shows its types across its bar, or as a strip clear of its label", () => {
  // 1_000_000 is the chart height of 200 px.
  expect(drawn("ranked", 100_000)).toEqual(["type.video", "type.document"]);
  // 100 px tall: the strip sits below the label.
  expect(drawn(undefined, 500_000)).toEqual(["type.video", "type.document"]);
  // 20 px: too low for a strip.
  expect(drawn(undefined, 100_000)).toEqual([]);
  // 40 px with a label: the strip would run into it.
  expect(drawn(undefined, 200_000)).toEqual([]);
});

test("README 54: a closed folder with its slices says its types, and the legend drops the uncoloured-folder note", () => {
  const input = modelFixture();
  const model = input.frame.model;
  model.representation = { ...model.representation, colorCriterion: "type" };
  const keys = ["type.document", "type.video", "type.document", "type.folder"];
  model.shapes.forEach((shape, index) => {
    shape.legendClassKey = keys[index]!;
  });
  model.legend = [
    {
      key: "type.document",
      labelKey: "ui.visualization.legend.type.document",
      itemCount: { unit: "items", state: "exact", value: "1" },
      total: model.totals.scope,
      condition: null,
    },
    {
      key: "type.video",
      labelKey: "ui.visualization.legend.type.video",
      itemCount: { unit: "items", state: "exact", value: "1" },
      total: model.totals.scope,
      condition: null,
    },
  ];
  const folder = model.shapes[3]!;
  folder.itemKind = "folder";
  folder.typeSlices = [slice("type.video", 1)];
  const row = input.page.response.rows.find(
    (candidate) => candidate.itemRef?.itemNo === folder.itemRef?.itemNo,
  );
  if (row) {
    row.itemKind = "folder";
    row.typeSlices = [slice("type.video", 1), slice("type.document", 1)];
  }
  render(
    <VisualizationModelPresenter
      frame={input.frame}
      page={input.page}
      view={{
        viewport: { width: 960, height: 330, scale: 1 },
        visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
      }}
      selected={null}
      onSelect={() => {}}
      legend={{
        criterion: <div>criterion</div>,
        patterns: false,
        onPatterns: () => {},
      }}
      text={text}
    />,
  );
  expect(screen.queryByText(say("legendFolderNote"))).toBeNull();
  const named = screen
    .getAllByRole("option")
    .map((option) => option.getAttribute("aria-label") ?? "");
  expect(
    named.some((name) => name.includes(`${say("legend.type.video")} 1 bytes`)),
  ).toBe(true);
  // The table names the folder by its largest type and how many more (54.6).
  expect(row).toBeDefined();
  act(() => {
    fireEvent.click(screen.getByRole("button", { name: say("rowsFromGraph") }));
  });
  expect(
    screen.getAllByText(
      say("typeSlicesMore")
        .replace("{name}", say("legend.type.video"))
        .replace("{count}", "1"),
    ).length,
  ).toBeGreaterThan(0);
});

test("README 54, 4.7: without narrowing, a chosen type keeps its slices bright in a folder and fades the rest", () => {
  const { container } = render(
    <TypeSlicesPreview scene="bar" narrows={false} />,
  );
  const legend = screen.getByRole("listbox", { name: say("legendTitle") });
  const video = Array.from(legend.querySelectorAll("[role='option']")).find(
    (option) => option.textContent?.startsWith(say("legend.type.video")),
  );
  act(() => {
    fireEvent.click(video!);
  });
  const work = container.querySelector(".du-slice-stack");
  // The folder itself is not faded: it holds the chosen type.
  expect(work?.closest("[data-dimmed]")).toBeNull();
  const faded = Array.from(work?.querySelectorAll("[data-key]") ?? []).map(
    (part) => [part.getAttribute("data-key"), part.hasAttribute("data-dimmed")],
  );
  expect(faded).toEqual([
    ["type.document", true],
    ["type.image", true],
    ["type.video", false],
  ]);
});
