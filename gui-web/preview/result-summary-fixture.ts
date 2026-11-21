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
import english from "../../user-env/i18n/en.json";

/*
 * Synthetic data for the result summary of the analysis screen. The product
 * shows this summary only for a scan it started; here a fixed result stands
 * in for one. Never imported by the product entry point.
 */

type Message = string | { kind: "plural"; variants: { other: string } };

function bundleText(bundle: unknown[]): LocaleText {
  const messages = new Map(
    ((bundle.find(Array.isArray) as [string, Message][] | undefined) ?? []).map(
      ([key, message]) => [key, message],
    ),
  );
  return (key, fallback, values) => {
    const message = messages.get(key);
    const template =
      message === undefined
        ? fallback
        : typeof message === "string"
          ? message
          : message.variants.other;
    return interpolate(template, (name) => String(values?.[name] ?? ""));
  };
}

/** Texts from the shipped Korean and English bundles for gallery previews. */
export const koreanText = bundleText(korean as unknown[]);
export const englishText = bundleText(english as unknown[]);

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

export const sharedLimitSummaryTotal: SpaceTotal = {
  ...summaryTotal,
  completeness: "missing",
  sizes: summaryTotal.sizes.map((entry) =>
    entry.size.basis === "allocated"
      ? {
          ...entry,
          size: { ...entry.size, state: "lower_bound", bytes: "2090000000000" },
          completeness: "missing",
          measurementLimits: [
            ...entry.measurementLimits,
            "space.allocated_may_include_shared",
          ],
        }
      : entry,
  ),
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
const typeSlice = (classKey: string, count: number, mebibytes: number) => ({
  classKey,
  itemCount: { unit: "items", state: "exact" as const, value: String(count) },
  total: {
    unit: "bytes",
    state: "exact" as const,
    value: String(mebibytes * 1024 * 1024),
  },
});

/** The classes of the fixed result: seven types, so the card names five and the rest. */
const typeSummary = {
  resultVersion: version,
  slices: [
    typeSlice("type.video", 12, 20),
    typeSlice("type.document", 640, 10),
    typeSlice("type.image", 2100, 7),
    typeSlice("type.archive", 18, 4),
    typeSlice("type.audio", 300, 3),
    typeSlice("type.no_extension", 90, 2),
    typeSlice("unclassified", 40, 1),
  ],
  legendOrder: [
    "type.document",
    "type.audio",
    "type.image",
    "type.installer_image",
    "type.video",
    "type.archive",
    "type.unclassified",
    "type.no_extension",
    "unclassified",
  ],
  lowerBound: false,
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
      case "space.type_summary":
        return typeSummary;
      default:
        throw new Error("The preview has no answer for " + name);
    }
  }) as AnalysisClient["call"],
  subscribe: () => () => undefined,
};
