import type {
  BrowseOpenResponse,
  FolderQueryRow,
  SizeValue,
  SpaceTotal,
} from "../src/ipc";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";

/*
 * Synthetic folder rows for the browse lower-bound states. Never imported by
 * the product entry.
 */

export const browseVersion = "0193f000-0000-7000-8000-000000000133";

/** What the result answers when it cannot prove a fact about a folder it read. */
const unproven = {
  state: "unknown",
  reasonKey: "space.data_insufficient",
} as const;
const read = { state: "present", value: false } as const;
const notRead = { state: "present", value: true } as const;

const size = (
  basis: "logical" | "allocated",
  bytes: string,
  lowerBound = false,
): { state: "present"; value: SizeValue } => ({
  state: "present",
  value: {
    basis,
    evidence: "os_reported",
    state: lowerBound ? "lower_bound" : "exact",
    bytes,
  },
});

function row(
  itemNo: number,
  name: string,
  kind: "directory" | "file",
  bytes: string,
  flags: FolderQueryRow["flags"],
  lowerBound = false,
): FolderQueryRow {
  return {
    item: { resultVersion: browseVersion, itemNo },
    displayName: { state: "present", value: name },
    kind,
    logicalSize: size("logical", bytes, lowerBound),
    allocatedSize: size("allocated", bytes, lowerBound),
    modifiedAt: { state: "present", value: "2026-11-20T09:00:00.000Z" },
    hasChildFolder: { state: "present", value: kind === "directory" },
    flags,
    correspondence: { state: "absent" },
  };
}

const folder = (
  boundary: NonNullable<FolderQueryRow["flags"]["boundary"]>,
  unread = false,
  unreadInside?: NonNullable<FolderQueryRow["flags"]["unreadInside"]>,
): FolderQueryRow["flags"] => ({
  unenumerated: unread ? notRead : unproven,
  excluded: read,
  boundary,
  ...(unreadInside ? { unreadInside } : {}),
});

/** Ancestor mark, self unread, and an exact folder in one table. */
export const browseRows: FolderQueryRow[] = [
  row(
    1,
    "상위 폴더",
    "directory",
    "734003200",
    folder(unproven, false, {
      state: "present",
      value: { count: "3" },
    }),
    true,
  ),
  row(
    2,
    "권한 없는 폴더",
    "directory",
    "1048576",
    folder(unproven, true),
    true,
  ),
  row(3, "정확한 크기 폴더", "directory", "48213", folder(unproven)),
];

const opened: Extract<BrowseOpenResponse, { kind: "opened" }> = {
  kind: "opened",
  requestSeq: 0,
  origin: "scan",
  location: { resultVersion: browseVersion, itemNo: 0 },
  breadcrumbs: [
    {
      item: { resultVersion: browseVersion, itemNo: 0 },
      displayName: { state: "present", value: "C:\\Users\\민서\\문서" },
    },
  ],
  rootKind: "scope_root",
  currentCorrespondence: false,
  page: {
    rowKind: "folder",
    queryRef: "0193f000-0000-7000-8000-000000000134",
    resultVersion: browseVersion,
    rows: browseRows,
    total: { state: "exact", count: browseRows.length },
    unknownCount: 0,
    observedAt: "2026-11-20T09:00:00.000Z",
    completeness: "complete",
    nextCursor: null,
  },
  sortStrategies: [],
  predicateStrategies: [],
};

/** Opens the fixed folder and a result-level size summary; nothing reaches an engine. */
export function browsePort(notice = false): BrowsePort {
  return {
    open: async (request) => ({
      ...opened,
      requestSeq: request.requestSeq,
      page: notice
        ? {
            ...opened.page,
            completeness: "partial",
            unexplainedLowerBound: "scan.evidence_not_recorded",
          }
        : opened.page,
    }),
    page: () => Promise.reject(new Error("The preview has one page")),
    spaceTotal: async (resultVersion): Promise<SpaceTotal> => {
      const size = (
        basis: SizeValue["basis"],
        lowerBound = false,
      ): SizeValue => ({
        basis,
        evidence: "os_reported",
        state: lowerBound ? "lower_bound" : "exact",
        bytes: "734003200",
      });
      return {
        resultVersion,
        requestSeq: 1,
        sizes: [
          {
            size: size("logical"),
            completeness: "complete",
            measurementLimits: [],
          },
          {
            size: size("allocated", notice),
            completeness: notice ? "partial" : "complete",
            measurementLimits: notice ? ["space.dedup_capacity"] : [],
          },
          {
            size: size("unique_physical", notice),
            completeness: notice ? "partial" : "complete",
            measurementLimits: notice ? ["space.dedup_capacity"] : [],
          },
        ],
        completeness: notice ? "partial" : "complete",
        missingItems: null,
        inaccessibleRanges: null,
        untraversedLinks: null,
      };
    },
  };
}
