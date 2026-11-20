import type {
  AnalysisClient,
  HistoryItemOutcome,
  HistoryItemRow,
  HistoryItemsResponse,
} from "../src/ipc";

/*
 * Synthetic item results of one recorded operation for the operation history
 * (F02 README #170). The app records no item results yet, so the gallery is
 * where the location cell can be seen. Never imported by the product entry.
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
  observedSize: string,
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
    observedSizeState: "exact",
    displaced: null,
  },
});

const rows = [
  row("1", historyLocations.short, "completed", "48213"),
  row("2", historyLocations.long, "completed", "1048576"),
  row("3", historyLocations.edge, "skipped", "912"),
  row("4", historyLocations.longName, "completed", "2097152"),
];

/** Answers the item results read; nothing reaches an engine. */
export const historyClient: AnalysisClient = {
  call: (async (
    name: string,
    body: { outcome?: HistoryItemOutcome | null },
  ) => {
    if (name !== "history.items")
      throw new Error("The preview has no answer for " + name);
    const shown = rows.filter(
      (item) => !body.outcome || item.result?.outcome === body.outcome,
    );
    const page: HistoryItemsResponse = {
      revision: { deleteGeneration: "0", maxRecordSeq: "4" },
      rows: shown,
      counts: { summary: "4", recorded: "4", unavailable: "0" },
      completeness: "complete",
      missingReasons: [],
      nextCursor: null,
    };
    return page;
  }) as AnalysisClient["call"],
  subscribe: () => () => undefined,
};
