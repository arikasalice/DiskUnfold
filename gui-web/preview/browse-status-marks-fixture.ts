import type { BrowseOpenResponse, FolderQueryRow, SizeValue } from "../src/ipc";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";

/*
 * Synthetic folder rows of one finished scan for the browse table's "Scan
 * status" column (F02 README "#133 「스캔 상태」 열"). A runner cannot make a
 * folder on another volume or one counted at another path, so the gallery
 * shows every mark together. Never imported by the product entry.
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
): FolderQueryRow["flags"] => ({
  unenumerated: unread ? notRead : unproven,
  excluded: read,
  boundary,
});

/** One row a mark, a folder read normally and a file, in one table. */
export const browseRows: FolderQueryRow[] = [
  row(1, "프로젝트", "directory", "734003200", folder(unproven)),
  row(2, "권한 없는 폴더", "directory", "4096", folder(unproven, true), true),
  row(
    3,
    "취소로 남은 폴더",
    "directory",
    "1048576",
    folder(unproven, true),
    true,
  ),
  row(
    4,
    "외장 디스크 연결",
    "directory",
    "0",
    folder({ state: "present", value: "other_volume" }),
  ),
  row(
    5,
    "사진 (다른 경로)",
    "directory",
    "0",
    folder({ state: "present", value: "already_counted" }),
  ),
  row(6, "보고서.docx", "file", "48213", {
    unenumerated: { state: "absent" },
    excluded: read,
  }),
];

const opened: BrowseOpenResponse = {
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
    completeness: "partial",
    nextCursor: null,
  },
  sortStrategies: [],
  predicateStrategies: [],
};

/** Opens the fixed folder; nothing reaches an engine. */
export const browsePort: BrowsePort = {
  open: async (request) => ({ ...opened, requestSeq: request.requestSeq }),
  page: () => Promise.reject(new Error("The preview has one page")),
};
