import type {
  AnalysisClient,
  HistoryItemOutcome,
  HistoryItemRow,
  HistoryItemsResponse,
} from "../src/ipc";

/*
 * Synthetic item results of one recorded operation for the operation history
 * (F02 README #170), plus the count sentences and empty states of #204. The
 * app records no item results yet, so the gallery is where they can be seen.
 * Never imported by the product entry.
 */

/**
 * The locations the design asked for: short, cut in the middle, edge space,
 * and a file name longer than the cell at 700 px.
 */
export const historyLocations = {
  short: "C:\\Users\\민서\\보고서.docx",
  long: "C:\\Users\\민서\\문서\\프로젝트\\2026 상반기 사업 보고서\\최종 검토본 모음\\부록 자료와 원본 데이터\\설문 응답 정리본.xlsx",
  edge: "C:\\Users\\민서\\문서\\회의록.txt ",
  longName:
    "C:\\Users\\민서\\문서\\2026년 상반기 고객 설문 응답 원본 정리본.xlsx",
};

/** A value outside a table whose last component alone is wider than its line. */
export const standaloneLocation =
  "D:\\보관\\사진\\2026 가족 여행 사진 원본과 보정본 전체 모음.zip";

const row = (
  itemSeq: string,
  locationDisplay: string,
  outcome: HistoryItemOutcome,
  observedSize: string | null,
  observedSizeState: "exact" | "lower_bound" | "unknown" = "exact",
): HistoryItemRow => ({
  itemSeq,
  result: {
    locationDisplay,
    outcome,
    reasonKey: null,
    reviewNeeded: false,
    held: false,
    outcomeUnknown: false,
    observedSize,
    observedSizeState,
    displaced: null,
  },
});

// #204: an exact size, an exact 1 MiB, a lower bound and a missing size.
const rows = [
  row("1", historyLocations.short, "completed", "48213"),
  row("2", historyLocations.long, "completed", "1048576"),
  row("3", historyLocations.edge, "skipped", "5242880", "lower_bound"),
  row("4", historyLocations.longName, "completed", null, "unknown"),
];

type ItemRecord = Pick<
  HistoryItemsResponse,
  "rows" | "counts" | "itemRecording" | "completeness" | "missingReasons"
>;

/** The records of the gallery by job reference, with the case each one shows. */
export const historyRecords: Record<string, ItemRecord> = {
  "preview-job": {
    rows,
    counts: { summary: "4", recorded: "4", unavailable: "0" },
    itemRecording: "recorded",
    completeness: "complete",
    missingReasons: [],
  },
  "preview-partial": {
    rows: [row("2", historyLocations.short, "completed", "48213")],
    counts: { summary: "5", recorded: "1", unavailable: "4" },
    itemRecording: "recorded",
    completeness: "partial",
    missingReasons: ["items_incomplete"],
  },
  "preview-total-unknown": {
    rows: [
      row("0", historyLocations.short, "completed", "48213"),
      row("1", historyLocations.edge, "failed", null, "unknown"),
    ],
    counts: { summary: null, recorded: "2", unavailable: null },
    itemRecording: "recorded",
    completeness: "partial",
    missingReasons: ["items_incomplete"],
  },
  "preview-not-recorded": {
    rows: [],
    counts: { summary: null, recorded: "0", unavailable: null },
    itemRecording: "not_recorded",
    completeness: "complete",
    missingReasons: [],
  },
  "preview-none-processed": {
    rows: [],
    counts: { summary: "0", recorded: "0", unavailable: "0" },
    itemRecording: "recorded",
    completeness: "complete",
    missingReasons: [],
  },
};

/** Answers the item results read by job reference; nothing reaches an engine. */
export const historyClient: AnalysisClient = {
  call: (async (
    name: string,
    body: { jobRef: string; outcome?: HistoryItemOutcome | null },
  ) => {
    if (name !== "history.items")
      throw new Error("The preview has no answer for " + name);
    const record = historyRecords[body.jobRef];
    if (!record) throw new Error("The preview has no record " + body.jobRef);
    const shown = record.rows.filter(
      (item) => !body.outcome || item.result?.outcome === body.outcome,
    );
    const page: HistoryItemsResponse = {
      ...record,
      revision: { deleteGeneration: "0", maxRecordSeq: "4" },
      rows: shown,
      nextCursor: null,
    };
    return page;
  }) as AnalysisClient["call"],
  subscribe: () => () => undefined,
};
