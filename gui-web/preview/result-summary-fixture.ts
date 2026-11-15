import { interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  BrowseDetail,
  ScopeExclusion,
  SpaceTotal,
  SpaceTotalSize,
  VolumeGapResponse,
} from "../src/ipc";
import korean from "../../user-env/i18n/ko.json";

/*
 * Synthetic data for the result summary of the analysis screen. The product
 * shows this summary only for a scan it started; here a fixed result stands
 * in for one. Never imported by the product entry point.
 */

type Message = string | { kind: "plural"; variants: { other: string } };

const messages = new Map(
  ((korean as unknown[]).find(Array.isArray) as [string, Message][]).map(
    ([key, message]) => [key, message],
  ),
);

/** Texts of the Korean bundle the product ships; Korean has one plural form. */
export const koreanText: LocaleText = (key, fallback, values) => {
  const message = messages.get(key);
  const template =
    message === undefined
      ? fallback
      : typeof message === "string"
        ? message
        : message.variants.other;
  return interpolate(template, (name) => String(values?.[name] ?? ""));
};

const version = "preview-result";
const observedAt = "2026-11-15T09:00:00.000Z";
const root = windowsPath("C:\\Users\\민서\\문서");

export const summaryRoots = [root];

const pathRule = (path: string): ScopeExclusion => ({
  kind: "path",
  path: windowsPath(path),
});

const namePattern = (pattern: string): ScopeExclusion => ({
  kind: "name_pattern",
  pattern,
});

/** The states of the summary's exclusions row; undefined is an unknown scope. */
export const exclusionStates = {
  two: [
    pathRule("C:\\Users\\민서\\문서\\보관함"),
    pathRule("C:\\Users\\민서\\문서\\임시 파일, 2026"),
  ],
  five: [
    pathRule("C:\\Users\\민서\\문서\\보관함"),
    namePattern("*.tmp"),
    pathRule("C:\\Users\\민서\\문서\\임시 파일, 2026"),
    pathRule("C:\\Users\\민서\\문서\\프로젝트\\2026 보고서\\빌드 산출물"),
    namePattern("node_modules"),
  ],
  unknown: undefined,
} satisfies Record<string, readonly ScopeExclusion[] | undefined>;

export type ExclusionState = keyof typeof exclusionStates;

export const exclusionStateLabels: Record<ExclusionState, string> = {
  two: "규칙 2개",
  five: "규칙 5개",
  unknown: "범위를 모름 (확인 불가)",
};

const size = (
  basis: "logical" | "allocated",
  bytes: string,
): SpaceTotalSize => ({
  size: { basis, evidence: "os_reported", state: "exact", bytes },
  completeness: "complete",
  measurementLimits: [],
});

const logical = size("logical", "52428800");
const allocated = size("allocated", "53686272");

export const summaryTotal: SpaceTotal = {
  resultVersion: version,
  requestSeq: 1,
  completeness: "complete",
  sizes: [logical, allocated],
  missingItems: "0",
  inaccessibleRanges: "0",
  untraversedLinks: "0",
};

const folder: BrowseDetail = {
  item: { resultVersion: version, itemNo: 0 },
  displayPath: { state: "present", value: root.display },
  displayName: { state: "present", value: "문서" },
  kind: { state: "present", value: "directory" },
  logicalSize: { state: "present", value: logical.size },
  allocatedSize: { state: "present", value: allocated.size },
  createdAt: { state: "absent" },
  modifiedAt: { state: "absent" },
  observedAt: { state: "present", value: observedAt },
  origin: "scan",
  completeness: "complete",
  currentCorrespondence: false,
  observationOnly: true,
};

const volume: VolumeGapResponse = {
  resultVersion: version,
  requestSeq: 0,
  volumeId: "AQ",
  start: { state: "not_attempted" },
  end: {
    state: "available",
    observedAt: null,
    totalBytes: "1073741824",
    usedBytes: "536870912",
    freeBytes: "536870912",
    evidence: "os_reported",
  },
  allocated: { size: allocated.size, completeness: "complete" },
  uniquePhysical: {
    size: {
      basis: "unique_physical",
      state: "exact",
      bytes: "53686272",
      evidence: "measured",
    },
    completeness: "complete",
  },
  factors: [],
  remainder: { state: "unknown", reasonKey: "space.data_insufficient" },
  osCategories: { state: "available", rows: [] },
};

/** Answers the reads the summary can make; nothing reaches an engine. */
export const summaryClient: AnalysisClient = {
  call: (async (name: string, body: { requestSeq?: number }) => {
    switch (name) {
      case "scan.failures":
        return { resultVersion: version, rows: [], totalCount: 0 };
      case "space.volume_gap":
        return { ...volume, requestSeq: body.requestSeq ?? 0 };
      case "browse.detail":
        return folder;
      default:
        throw new Error("The preview has no answer for " + name);
    }
  }) as AnalysisClient["call"],
  subscribe: () => () => undefined,
};
