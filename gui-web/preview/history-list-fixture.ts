import type {
  AnalysisClient,
  HistoryDetail,
  HistoryItemsResponse,
  HistoryListResponse,
  HistoryListRow,
  HistoryListSummary,
} from "../src/ipc";

/* Fixed scope-root records for the history-list gallery; never used by product code. */
const revision = { deleteGeneration: "0", maxRecordSeq: "4" };

function summary(finishedAtMs: number, roots?: string[]): HistoryListSummary {
  return {
    kindId: "scan.metadata",
    kindVersion: 1,
    scopeSummary: null,
    ...(roots
      ? { scopeFirstRoot: roots[0]!, scopeRootCount: roots.length }
      : {}),
    state: "done",
    counts: {
      completed: "12",
      overwritten: "0",
      failed: "0",
      skipped: "0",
      unprocessed: "0",
    },
    startedAtMs: finishedAtMs - 60_000,
    finishedAtMs,
    predecessorJobRef: null,
    relation: null,
    sessionRestoreAvailable: null,
  };
}

const roots = {
  one: ["C:\\Users\\민서\\Videos"],
  many: [
    "C:\\Users\\민서\\문서\\프로젝트\\2026 상반기 사업 보고서\\최종 검토본 모음\\Videos",
    "D:\\보관\\가져온 파일",
    "E:\\보관함\\2026년 상반기 고객 설문 응답 원본과 정리본 전체 모음\\부록 자료와 원본 데이터\\설문 응답 정리본 최종",
  ],
  trailingSpace: ["D:\\보관\\사진 "],
};

const rows: HistoryListRow[] = [
  {
    jobRef: "scope-one",
    summary: summary(Date.UTC(2026, 10, 20, 9, 15), roots.one),
    missingReasons: [],
  },
  {
    jobRef: "scope-many",
    summary: summary(Date.UTC(2026, 10, 20, 8, 45), roots.many),
    missingReasons: [],
  },
  {
    jobRef: "scope-edge-space",
    summary: summary(Date.UTC(2026, 10, 20, 8, 10), roots.trailingSpace),
    missingReasons: [],
  },
  {
    jobRef: "scope-not-recorded",
    summary: summary(Date.UTC(2026, 10, 20, 7, 30)),
    missingReasons: [],
  },
];

function detail(row: HistoryListRow): HistoryDetail {
  return {
    summary: row.summary,
    ...(row.jobRef === "scope-many" ? { scopeRoots: roots.many } : {}),
    workflow: { planVersions: null, executedPlan: null, approvalCheck: null },
    followUps: null,
    effects: { estimated: null, executed: null, confirmed: null },
    completeness: "complete",
    missingReasons: [],
  };
}

const details = new Map(rows.map((row) => [row.jobRef, detail(row)]));

const notRecordedItems: HistoryItemsResponse = {
  revision,
  rows: [],
  counts: { summary: null, recorded: "0", unavailable: null },
  itemRecording: "not_recorded",
  completeness: "complete",
  missingReasons: [],
  nextCursor: null,
};

/** Responds only with local gallery data; unrelated commands intentionally fail. */
export const historyListClient: AnalysisClient = {
  call: (async (name: string, body: { jobRef?: string }) => {
    if (name === "history.list") {
      const page: HistoryListResponse = {
        revision,
        rows,
        nextCursor: null,
      };
      return page;
    }
    if (name === "history.get") {
      const value = details.get(body.jobRef ?? null);
      if (!value) throw new Error("The preview has no history detail");
      return value;
    }
    if (name === "history.items") return notRecordedItems;
    throw new Error("The preview has no answer for " + name);
  }) as AnalysisClient["call"],
  subscribe: () => () => undefined,
};
