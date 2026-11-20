import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import { scanStatus, size } from "../src/features/browse/browse-cells";
import type { LocaleText } from "../src/i18n/presentation";
import type {
  BrowseOpenResponse,
  FolderQueryPage,
  FolderQueryRow,
} from "../src/ipc";
import { BrowsePages } from "../src/state/browse-pages";

afterEach(cleanup);

const text: LocaleText = (_key, fallback) => fallback;
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

function opened(rows: FolderQueryRow[]): BrowseOpenResponse {
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

function show(response: BrowseOpenResponse) {
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(response),
    page: vi.fn(),
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
});

test("a response without the boundary has no scan status column", async () => {
  show(opened([row(1, {})]));
  expect(await screen.findByText("Folder 1")).toBeDefined();
  expect(
    screen.queryByRole("columnheader", { name: "Scan status" }),
  ).toBeNull();
});
