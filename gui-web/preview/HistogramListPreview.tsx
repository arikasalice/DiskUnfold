import { useLayoutEffect, useRef, useState } from "react";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { TypeList } from "../src/features/visualization/TypeList";
import type { TypeListSource } from "../src/features/visualization/TypeList";
import {
  intervalListing,
  intervalRow,
} from "../src/features/visualization/interval-list";
import type { IntervalListing } from "../src/features/visualization/interval-list";
import type {
  QueryCondition,
  SearchQueryRow,
  VizRow,
  VizShape,
} from "../src/ipc";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import { modelFixture } from "./model-fixture";

/**
 * A histogram whose intervals open their file list (F06 README 52, #297): the
 * bar, Enter, and the table's button open it in the chart's place, and going
 * back returns the focus. Synthetic values; no product command reads them.
 */
const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
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
const KiB = 1024;
const bounds: [number, number | undefined, string, string][] = [
  [0, KiB, "0 B–1 KiB", "12"],
  [KiB, KiB ** 2, "1 KiB–1 MiB", "31"],
  [KiB ** 2, KiB ** 3, "1 MiB–1 GiB", "6"],
  [KiB ** 3, undefined, "1 GiB 이상", "2"],
];
const sizes = ["5120", "9437184", "24117248", "3221225472"];
const size = (op: "at_least" | "less_than", bytes: number): QueryCondition => ({
  kind: "predicate",
  predicate: { kind: "size", basis: "allocated", op, bytes: String(bytes) },
});

function fixture() {
  const { frame, page } = modelFixture();
  frame.completeness = "complete";
  page.response.completeness = "complete";
  const model = frame.model;
  model.representation = {
    ...model.representation,
    format: "histogram",
    measure: "count",
    sizeBasis: "allocated",
    colorMode: null,
    labelMode: null,
  };
  const top = Math.max(...bounds.map(([, , , count]) => Number(count)));
  const base = model.shapes[0]!;
  model.shapes = bounds.map(([, , , count], index): VizShape => ({
    ...base,
    itemRef: null,
    groupRef: "bin-" + index,
    shapeRef: "bin-" + index,
    rootBranchRef: null,
    quantity: { unit: "items", state: "exact", value: count },
    memberCount: { unit: "items", state: "exact", value: count },
    size: {
      basis: "allocated",
      evidence: "os_reported",
      state: "exact",
      bytes: sizes[index]!,
    },
    geometry: {
      primitive: "rect",
      points: [
        {
          x: index * 250000 + 4000,
          y: 1000000 - (Number(count) / top) * 1000000,
        },
        { x: (index + 1) * 250000 - 4000, y: 1000000 },
      ],
    },
  }));
  page.response.rows = model.shapes.map((shape, index): VizRow => {
    const [lower, upper, name] = bounds[index]!;
    return {
      ...page.response.rows[0]!,
      itemRef: null,
      groupRef: shape.groupRef,
      displayName: name,
      size: shape.size,
      ...(shape.quantity ? { quantity: shape.quantity } : {}),
      ...(shape.memberCount ? { memberCount: shape.memberCount } : {}),
      ratio: null,
      condition: {
        kind: "all",
        children: [
          { kind: "predicate", predicate: { kind: "type", value: "file" } },
          size("at_least", lower),
          ...(upper === undefined ? [] : [size("less_than", upper)]),
        ],
      },
    };
  });
  page.response.totalCount = bounds.length;
  return { frame, page };
}

const ROOT = "D:\\사진";
function file(itemNo: number, name: string, bytes: string): SearchQueryRow {
  const value = { evidence: "os_reported", state: "exact", bytes } as const;
  return {
    item: { resultVersion: modelFixture().frame.resultRevision, itemNo },
    displayName: { state: "present", value: name },
    displayPath: { state: "present", value: ROOT + "\\" + name },
    kind: "file",
    logicalSize: { state: "present", value: { ...value, basis: "logical" } },
    allocatedSize: {
      state: "present",
      value: { ...value, basis: "allocated" },
    },
    modifiedAt: {
      state: "present",
      value: "2026-10-" + (10 + itemNo) + "T09:30:00Z",
    },
    createdAt: { state: "absent" },
    missingFields: [],
    reasonKeys: [],
  };
}
const files = [
  file(11, "가족여행.mov", "9437184"),
  file(12, "졸업식.mp4", "5242880"),
  file(13, "발표 녹화.mp4", "3145728"),
  file(14, "스캔본.pdf", "2097152"),
  file(15, "포스터.psd", "2097152"),
  file(16, "악보.pdf", "1048576"),
];
const source: TypeListSource = {
  open: async () => ({
    page: {
      rowKind: "search",
      queryRef: "preview-interval",
      resultVersion: modelFixture().frame.resultRevision,
      rows: files,
      total: { state: "exact", count: files.length },
      unknownCount: 0,
      observedAt: null,
      completeness: "complete",
      nextCursor: null,
    },
    scopePath: ROOT,
  }),
  next: () => Promise.reject(new Error("one page")),
};

export function HistogramListPreview() {
  const [{ frame, page }] = useState(fixture);
  const [listing, setListing] = useState<IntervalListing | null>(null);
  const returning = useRef<IntervalListing["origin"]>(null);
  useLayoutEffect(() => {
    if (listing || !returning.current) return;
    returning.current.focus();
    returning.current = null;
  }, [listing]);
  const open = (groupRef: string, origin: HTMLElement | SVGElement | null) => {
    const row = intervalRow(page, groupRef);
    const next = row && intervalListing(row, frame, text, origin);
    if (next) setListing(next);
  };
  const focused = () =>
    document.activeElement instanceof SVGElement ||
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  return (
    <main className="du-histogram-list-preview" style={{ padding: 24 }}>
      {listing && (
        <TypeList
          source={source}
          resultVersion={frame.resultRevision}
          scope={frame.model.aggregateScopeRef}
          condition={listing.condition}
          type={listing.label}
          interval={listing.label}
          summary={listing.summary}
          text={text}
          onBack={() => {
            returning.current = listing.origin;
            setListing(null);
          }}
        />
      )}
      <div hidden={!!listing}>
        <VisualizationModelPresenter
          frame={frame}
          page={page}
          view={{
            viewport: {
              width: Math.max(320, window.innerWidth - 48),
              height: 360,
              scale: 1,
            },
            visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
          }}
          selected={null}
          onSelect={(selection) => {
            if (selection.kind === "group") open(selection.groupRef, focused());
          }}
          onActivate={(selection) => {
            if (selection.kind === "group") open(selection.groupRef, focused());
          }}
          onOpenGroup={(groupRef, _size, origin) => open(groupRef, origin)}
          text={text}
        />
      </div>
    </main>
  );
}
