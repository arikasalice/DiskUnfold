import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import {
  browseNotice,
  hasDedupCapacityLowerBound,
  scanStatus,
  size,
} from "../src/features/browse/browse-cells";
import type { LocaleText } from "../src/i18n/presentation";
import { interpolate } from "../src/i18n/presentation";
import type {
  BrowseOpenResponse,
  FolderQueryPage,
  FolderQueryRow,
  SizeValue,
  SpaceTotal,
} from "../src/ipc";
import { BrowsePages } from "../src/state/browse-pages";

afterEach(cleanup);

const text: LocaleText = (_key, fallback, values) =>
  interpolate(fallback, (name) => String(values?.[name] ?? "Unknown"));
const id = "018f0000-0000-7000-8000-000000000002";
type Flags = FolderQueryRow["flags"];
const inProgress = { state: "unknown", reasonKey: "scan.in_progress" } as const;
/** What the result answers when it cannot prove a fact about a folder it read. */
const unproven = {
  state: "unknown",
  reasonKey: "space.data_insufficient",
} as const;
const unread = { state: "present", value: true } as const;
const read = { state: "present", value: false } as const;
const boundary = (value: string) => ({ state: "present", value }) as const;

function row(
  n: number,
  flags: Partial<Flags>,
  kind = "directory",
): FolderQueryRow {
  return {
    item: { resultVersion: id, itemNo: n },
    displayName: { state: "present", value: "Folder " + n },
    kind: kind as FolderQueryRow["kind"],
    logicalSize: { state: "absent" },
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: { state: "absent" },
    flags: { unenumerated: read, excluded: read, ...flags },
    correspondence: { state: "absent" },
  };
}

function page(rows: FolderQueryRow[], count = rows.length): FolderQueryPage {
  return {
    rowKind: "folder",
    queryRef: id,
    resultVersion: id,
    rows,
    total: { state: "exact", count },
    unknownCount: 0,
    observedAt: null,
    completeness: "complete",
    nextCursor: null,
  };
}

test.each([
  [
    "a folder confirmed not to be a boundary",
    { boundary: { state: "absent" } },
    "",
  ],
  [
    "a folder read normally, whose boundary the result cannot prove",
    { unenumerated: unproven, boundary: unproven },
    "",
  ],
  [
    "a folder the running scan has not reached",
    { unenumerated: inProgress, boundary: inProgress },
    "",
  ],
  [
    "a folder whose contents were not read",
    { unenumerated: unread, boundary: unproven },
    "Contents not read",
  ],
  [
    "a folder on another volume",
    { boundary: boundary("other_volume") },
    "Other volume",
  ],
  [
    "a folder counted at another path",
    { boundary: boundary("already_counted") },
    "Counted at another path",
  ],
  [
    "a folder with two marks",
    { unenumerated: unread, boundary: boundary("other_volume") },
    "Contents not read · Other volume",
  ],
  [
    "a boundary this version has no name for",
    { boundary: boundary("future_kind") },
    "",
  ],
] as [string, Partial<Flags>, string][])(
  "scan status of %s",
  (_, flags, expected) => {
    expect(scanStatus(row(1, flags), text)).toBe(expected);
  },
);

test("a file row, which carries no boundary, has a blank scan status", () => {
  expect(scanStatus(row(1, {}, "file"), text)).toBe("");
});

test.each([
  ["omitted", undefined, ""],
  ["unknown while scanning", inProgress, ""],
  ["absent", { state: "absent" }, ""],
  [
    "one unread range",
    { state: "present", value: { count: "1" } },
    "1 location inside not read",
  ],
  [
    "several unread ranges",
    { state: "present", value: { count: "3" } },
    "3 locations inside not read",
  ],
  [
    "an uncounted unread range",
    { state: "present", value: {} },
    "Some locations inside not read",
  ],
] as [string, Flags["unreadInside"] | undefined, string][])(
  "unreadInside mark for %s",
  (_case, unreadInside, expected) => {
    expect(
      scanStatus(
        row(1, unreadInside === undefined ? {} : { unreadInside }),
        text,
      ),
    ).toBe(expected);
  },
);

test("a folder not read itself keeps only its existing mark", () => {
  expect(
    scanStatus(
      row(1, {
        unenumerated: unread,
        unreadInside: { state: "present", value: { count: "3" } },
      }),
      text,
    ),
  ).toBe("Contents not read");
});

test("unread ranges join other row marks with the existing middle dot", () => {
  expect(
    scanStatus(
      row(1, {
        boundary: boundary("other_volume"),
        unreadInside: { state: "present", value: { count: "3" } },
      }),
      text,
    ),
  ).toBe("3 locations inside not read · Other volume");
});

test("an unread-inside folder uses the existing minimum size presentation", () => {
  const value = {
    state: "present",
    value: {
      basis: "allocated",
      evidence: "os_reported",
      state: "lower_bound",
      bytes: "4096",
    },
  } as const;
  expect(
    size(
      row(1, { unreadInside: { state: "present", value: { count: "3" } } }),
      value,
      { system: "iec", unit: "auto" },
      text,
    ),
  ).toBe("At least 4.0 KiB");
});

test.each([
  [
    "scan.evidence_not_recorded",
    "This result doesn't record whether folders were read completely, so some sizes and counts are minimums. Analyze again to see exact values.",
  ],
  [
    "scan.evidence_budget_exceeded",
    "There were too many folders to record whether each was read completely. Sizes and counts for those folders are minimums.",
  ],
  [
    "scan.lower_bound_unexplained",
    "This result doesn't record whether folders were read completely, so some sizes and counts are minimums.",
  ],
  [
    "unknown.future_reason",
    "This result doesn't record whether folders were read completely, so some sizes and counts are minimums.",
  ],
])("lower-bound notice for %s", (key, expected) => {
  expect(browseNotice(false, [key], false, text)).toEqual([expected]);
});

test("the browse notice keeps two reasons on separate lines in order", () => {
  expect(
    browseNotice(false, ["scan.evidence_not_recorded"], true, text),
  ).toEqual([
    "This result doesn't record whether folders were read completely, so some sizes and counts are minimums. Analyze again to see exact values.",
    "There were too many files that may share content, so some weren't counted. Allocated and unique physical sizes are minimums.",
  ]);
});

test("the browse notice puts the incomplete headline before both reasons", () => {
  expect(
    browseNotice(true, ["scan.evidence_not_recorded"], true, text),
  ).toEqual([
    "Results are incomplete.",
    "This result doesn't record whether folders were read completely, so some sizes and counts are minimums. Analyze again to see exact values.",
    "There were too many files that may share content, so some weren't counted. Allocated and unique physical sizes are minimums.",
  ]);
});

test("the browse notice shows only the headline for an incomplete result", () => {
  expect(browseNotice(true, [], false, text)).toEqual([
    "Results are incomplete.",
  ]);
});

function spaceTotal(sizes: SpaceTotal["sizes"]): SpaceTotal {
  return {
    resultVersion: id,
    requestSeq: 1,
    sizes,
    completeness: "complete",
    missingItems: null,
    inaccessibleRanges: null,
    untraversedLinks: null,
  };
}

function totalSize(
  basis: SizeValue["basis"],
  lowerBound: boolean,
  measurementLimits: string[],
): SpaceTotal["sizes"][number] {
  return {
    size: {
      basis,
      evidence: "os_reported",
      state: lowerBound ? "lower_bound" : "exact",
      bytes: "4096",
    },
    completeness: "complete",
    measurementLimits,
  };
}

test.each(["allocated", "unique_physical"] as const)(
  "shows the file-identity notice only for a %s lower bound with its measurement limit",
  (basis) => {
    const limited = spaceTotal([
      totalSize(basis, true, ["space.dedup_capacity"]),
    ]);
    const exact = spaceTotal([
      totalSize(basis, false, ["space.dedup_capacity"]),
    ]);
    const unexplained = spaceTotal([totalSize(basis, true, [])]);
    expect(hasDedupCapacityLowerBound(limited)).toBe(true);
    expect(
      browseNotice(false, [], hasDedupCapacityLowerBound(limited), text),
    ).toEqual([
      "There were too many files that may share content, so some weren't counted. Allocated and unique physical sizes are minimums.",
    ]);
    expect(hasDedupCapacityLowerBound(exact)).toBe(false);
    expect(hasDedupCapacityLowerBound(unexplained)).toBe(false);
  },
);

test("no browse notice appears when no incomplete result or lower bound applies", () => {
  expect(browseNotice(false, [], false, text)).toBeUndefined();
});

test("a zero size of a folder the scan did not enter reads as not counted", () => {
  const display = { system: "iec", unit: "auto" } as const;
  const sized = (flags: Partial<Flags>, bytes: string) => {
    const value = {
      state: "present",
      value: { basis: "logical", evidence: "measured", state: "exact", bytes },
    } as const;
    return size({ ...row(1, flags), logicalSize: value }, value, display, text);
  };
  expect(sized({ boundary: boundary("other_volume") }, "0")).toBe(
    "Not counted",
  );
  expect(sized({ boundary: boundary("already_counted") }, "0")).toBe(
    "Not counted",
  );
  // A value other than zero is shown as it is, and an empty folder stays empty.
  expect(sized({ boundary: boundary("other_volume") }, "7")).toBe("7 bytes");
  expect(sized({ boundary: unproven }, "0")).toBe("0 bytes");
});

test("a lower bound of 0 bytes is unavailable only when something inside was not read", () => {
  const display = { system: "iec", unit: "auto" } as const;
  const sized = (flags: Partial<Flags>, bytes: string) => {
    const value = {
      state: "present",
      value: {
        basis: "logical",
        evidence: "measured",
        state: "lower_bound",
        bytes,
      },
    } as const;
    return size({ ...row(1, flags), logicalSize: value }, value, display, text);
  };
  const inside = { state: "present", value: { count: "1" } } as const;
  expect(sized({ unenumerated: unread }, "0")).toBe("Unavailable");
  expect(sized({ unreadInside: inside }, "0")).toBe("Unavailable");
  // Everything counted is empty: the lower bound is shown as it is.
  expect(sized({}, "0")).toBe("At least 0 bytes");
  expect(sized({ unenumerated: unread }, "2048")).toBe("At least 2.0 KiB");
});

test("a list keeps the scan status column once a row carried the boundary", () => {
  const files = Array.from({ length: 200 }, (_, n) => row(n, {}, "file"));
  const first = page(files, 201);
  first.nextCursor = "next";
  const pages = new BrowsePages(first);
  expect(pages.boundaries).toBe(false);
  expect(
    pages.accept(200, page([row(200, { boundary: unproven })], 201), new Set()),
  ).toBe(true);
  expect(pages.boundaries).toBe(true);
});

function opened(
  rows: FolderQueryRow[],
): Extract<BrowseOpenResponse, { kind: "opened" }> {
  return {
    kind: "opened",
    requestSeq: 1,
    origin: "scan",
    location: { resultVersion: id, itemNo: 0 },
    breadcrumbs: [
      {
        item: { resultVersion: id, itemNo: 0 },
        displayName: { state: "present", value: "Root" },
      },
    ],
    rootKind: "scope_root",
    currentCorrespondence: false,
    page: page(rows),
    sortStrategies: [],
    predicateStrategies: [],
    // Shown only after #217; until then the summary leaves it out.
    excludedChildren: { state: "exact", count: 3 },
  };
}

function show(
  response: BrowseOpenResponse,
  spaceTotal?: BrowsePort["spaceTotal"],
) {
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(response),
    page: vi.fn(),
    ...(spaceTotal ? { spaceTotal } : {}),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
}

const cells = (name: string) =>
  screen.getByText(name).closest<HTMLElement>('[role="row"]')!;

test("the browse table shows the recorded marks and leaves unproven folders blank", async () => {
  show(
    opened([
      row(1, { unenumerated: unread, boundary: unproven }),
      row(2, { boundary: boundary("other_volume") }),
      row(3, { unenumerated: unproven, boundary: unproven }),
    ]),
  );
  expect(
    await screen.findByRole("columnheader", { name: "Scan status" }),
  ).toBeDefined();
  expect(
    within(cells("Folder 1")).getByText("Contents not read"),
  ).toBeDefined();
  expect(within(cells("Folder 2")).getByText("Other volume")).toBeDefined();
  expect(
    within(cells("Folder 3")).queryByText(/Unknown|Contents not read/),
  ).toBeNull();
  expect(screen.queryByText(/excluded/)).toBeNull();
  expect(screen.queryByText("Results are incomplete.")).toBeNull();
});

test("a response without the boundary has no scan status column", async () => {
  show(opened([row(1, {})]));
  expect(await screen.findByText("Folder 1")).toBeDefined();
  expect(
    screen.queryByRole("columnheader", { name: "Scan status" }),
  ).toBeNull();
});

test("the browse notice uses one region with a separate line for each sentence", async () => {
  const response = opened([row(1, {})]);
  response.page.completeness = "partial";
  response.page.unexplainedLowerBound = "scan.evidence_not_recorded";
  const readTotal = vi
    .fn()
    .mockResolvedValue(
      spaceTotal([totalSize("allocated", true, ["space.dedup_capacity"])]),
    );
  show(response, readTotal);
  await screen.findByText(
    "There were too many files that may share content, so some weren't counted. Allocated and unique physical sizes are minimums.",
  );
  const headline = await screen.findByText("Results are incomplete.");
  const notice = headline.closest(".du-notice");
  const lines = notice?.querySelector(".du-browse-results__notice-lines");
  expect(notice?.getAttribute("role")).toBe("status");
  expect(
    document.querySelectorAll('.du-browse-results > [role="status"]'),
  ).toHaveLength(1);
  expect(
    Array.from(lines?.children ?? []).map((line) => line.textContent),
  ).toEqual([
    "Results are incomplete.",
    "This result doesn't record whether folders were read completely, so some sizes and counts are minimums. Analyze again to see exact values.",
    "There were too many files that may share content, so some weren't counted. Allocated and unique physical sizes are minimums.",
  ]);
  expect(readTotal).toHaveBeenCalledWith(id);
});
