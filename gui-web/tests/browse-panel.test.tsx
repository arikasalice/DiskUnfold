import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import type {
  BrowseOpenResponse,
  FolderQueryPage,
  FolderQueryRow,
} from "../src/ipc";

afterEach(cleanup);
const id = "018f0000-0000-7000-8000-000000000001";
const ref = (n: number) => ({ resultVersion: id, itemNo: n });
const row = (n: number, label: string): FolderQueryRow => ({
  item: ref(n),
  displayName: { state: "present", value: label },
  kind: "directory",
  logicalSize: { state: "unknown", reasonKey: "ref.unavailable" },
  allocatedSize: { state: "absent" },
  modifiedAt: { state: "absent" },
  hasChildFolder: { state: "unknown", reasonKey: "ref.unavailable" },
  flags: {
    unenumerated: { state: "unknown", reasonKey: "ref.unavailable" },
    excluded: { state: "absent" },
  },
  correspondence: { state: "unknown", reasonKey: "ref.unavailable" },
});
const page = (
  rows: FolderQueryRow[],
  cursor: string | null = null,
): FolderQueryPage => ({
  rowKind: "folder",
  queryRef: id,
  resultVersion: id,
  rows,
  total: { state: "exact", count: rows.length },
  unknownCount: 0,
  observedAt: null,
  completeness: "complete",
  nextCursor: cursor,
});
const opened = (
  seq: number,
  rows: FolderQueryRow[],
  cursor: string | null = null,
): BrowseOpenResponse => ({
  kind: "opened",
  requestSeq: seq,
  origin: "scan",
  location: null,
  breadcrumbs: [],
  rootKind: "virtual_root",
  currentCorrespondence: false,
  page: page(rows, cursor),
  sortStrategies: [],
  predicateStrategies: [],
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("shows initial loading, virtual root, partial data and next page", async () => {
  const initial = deferred<BrowseOpenResponse>();
  const later = deferred<FolderQueryPage>();
  const port: BrowsePort = {
    open: vi.fn(() => initial.promise),
    page: vi.fn(() => later.promise),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  expect(screen.getByLabelText("Opening folder")).toBeDefined();
  const first = opened(1, [row(1, "Alpha")], "token");
  if (first.kind !== "opened") throw new Error("wrong fixture");
  first.page.completeness = "partial";
  first.page.total = { state: "lower_bound", count: 1 };
  await act(async () => initial.resolve(first));
  expect(screen.getByText("Scope roots")).toBeDefined();
  expect(screen.getByText(/Results are incomplete/)).toBeDefined();
  expect(screen.getByRole("table").getAttribute("aria-rowcount")).toBe("-1");
  fireEvent.click(screen.getByRole("button", { name: "Load more" }));
  expect(port.page).toHaveBeenCalledWith({ nextCursor: "token", count: 200 });
  await act(async () => later.resolve(page([row(2, "Beta")])));
  expect(screen.getByRole("button", { name: "Beta" })).toBeDefined();
});

test("keeps the latest navigation and discards an older result", async () => {
  const pending = [
    deferred<BrowseOpenResponse>(),
    deferred<BrowseOpenResponse>(),
    deferred<BrowseOpenResponse>(),
  ] as const;
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockImplementationOnce(() => pending[0].promise)
      .mockImplementationOnce(() => pending[1].promise)
      .mockImplementationOnce(() => pending[2].promise),
    page: vi.fn(),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  await act(async () =>
    pending[0].resolve(opened(1, [row(1, "Alpha"), row(2, "Beta")])),
  );
  fireEvent.click(screen.getByRole("button", { name: "Alpha" }));
  fireEvent.click(screen.getByRole("button", { name: "Beta" }));
  await act(async () => pending[2].resolve(opened(3, [row(3, "Latest")])));
  await act(async () => pending[1].resolve(opened(2, [row(4, "Old")])));
  expect(screen.getByRole("button", { name: "Latest" })).toBeDefined();
  expect(screen.queryByRole("button", { name: "Old" })).toBeNull();
});

test("announces empty results, failure, and asynchronous work", async () => {
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(opened(1, []))
      .mockRejectedValueOnce(new Error("failed"))
      .mockImplementationOnce((request) =>
        Promise.resolve({
          kind: "job",
          requestSeq: request.requestSeq,
          jobRef: id,
        }),
      ),
    page: vi.fn(),
  };
  const { rerender } = render(<BrowsePanel port={port} resultVersion={id} />);
  expect(
    await screen.findByText("No observed items in this folder."),
  ).toBeDefined();
  rerender(<BrowsePanel port={port} resultVersion="second" />);
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Could not open",
  );
  rerender(<BrowsePanel port={port} resultVersion="third" />);
  expect(
    await screen.findByText("Folder results are being prepared."),
  ).toBeDefined();
});

test("keyboard opens a folder and breadcrumbs return to the virtual root", async () => {
  const port: BrowsePort = {
    open: vi.fn().mockImplementation((request) => {
      const response = opened(
        request.requestSeq,
        request.location ? [] : [row(1, "Alpha")],
      );
      if (response.kind === "opened" && request.location) {
        response.location = request.location;
        response.rootKind = "scope_root";
        response.breadcrumbs = [
          {
            item: request.location,
            displayName: { state: "present", value: "Alpha" },
          },
        ];
      }
      return Promise.resolve(response);
    }),
    page: vi.fn(),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const alpha = await screen.findByRole("button", { name: "Alpha" });
  alpha.focus();
  fireEvent.keyDown(alpha, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(alpha, { key: "Enter", code: "Enter" });
  expect(
    await screen.findByRole("button", { name: "Scope roots" }),
  ).toBeDefined();
  expect(document.activeElement).toBe(
    screen.getByRole("region", { name: "Folder navigation" }),
  );
  expect(port.open).toHaveBeenLastCalledWith(
    expect.objectContaining({ location: ref(1), resultVersion: id }),
    expect.any(AbortSignal),
  );
  fireEvent.click(screen.getByRole("button", { name: "Scope roots" }));
  expect(await screen.findByRole("button", { name: "Alpha" })).toBeDefined();
});

test("rejects a page from another result without discarding visible rows", async () => {
  const bad = page([row(2, "Wrong")]);
  bad.resultVersion = "other";
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(opened(1, [row(1, "Alpha")], "next")),
    page: vi.fn().mockResolvedValue(bad),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  fireEvent.click(await screen.findByRole("button", { name: "Load more" }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Could not load",
  );
  expect(screen.getByRole("button", { name: "Alpha" })).toBeDefined();
  expect(screen.queryByRole("button", { name: "Wrong" })).toBeNull();
});

test("discards a late page after navigating to another folder", async () => {
  const late = deferred<FolderQueryPage>();
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(opened(1, [row(1, "Alpha")], "next"))
      .mockResolvedValueOnce(opened(2, [row(3, "Current")])),
    page: vi.fn(() => late.promise),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  fireEvent.click(await screen.findByRole("button", { name: "Load more" }));
  fireEvent.click(screen.getByRole("button", { name: "Alpha" }));
  expect(await screen.findByRole("button", { name: "Current" })).toBeDefined();
  await act(async () => late.resolve(page([row(4, "Stale")])));
  expect(screen.queryByRole("button", { name: "Stale" })).toBeNull();
});

test.each(["directory", "file"] as const)(
  "keyboard paging focuses the first newly mounted %s when Load more is removed",
  async (kind) => {
    const pending = deferred<FolderQueryPage>();
    const first = opened(
      1,
      Array.from({ length: 200 }, (_, index) =>
        row(index + 1, "Existing " + index),
      ),
      "next",
    );
    if (first.kind !== "opened") throw new Error("wrong fixture");
    first.page.total = { state: "exact", count: 201 };
    const port: BrowsePort = {
      open: vi.fn().mockResolvedValue(first),
      page: vi.fn(() => pending.promise),
    };
    render(<BrowsePanel port={port} resultVersion={id} />);
    const load = await screen.findByRole("button", { name: "Load more" });
    expect(screen.queryByText("New item")).toBeNull();
    expect(screen.queryByText("Existing 199")).toBeNull();
    act(() => load.focus());
    fireEvent.keyDown(load, { key: "Enter", code: "Enter" });
    fireEvent.keyUp(load, { key: "Enter", code: "Enter" });
    expect(document.activeElement).toBe(load);
    fireEvent.click(load);
    expect(port.page).toHaveBeenCalledTimes(1);
    await act(async () =>
      pending.resolve(page([{ ...row(201, "New item"), kind }])),
    );
    const newItem = screen.getByText("New item");
    expect(load.isConnected).toBe(false);
    expect(document.activeElement).toBe(
      kind === "directory"
        ? screen.getByRole("button", { name: "New item" })
        : newItem.closest("tr"),
    );
    expect(newItem.closest("tr")?.getAttribute("aria-rowindex")).toBe("202");
    expect(screen.queryByText("Existing 199")).toBeNull();
  },
);

test("keyboard paging leaves focus where the user moved it before the final response", async () => {
  const pending = deferred<FolderQueryPage>();
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(opened(1, [row(1, "Alpha")], "next")),
    page: vi.fn(() => pending.promise),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const load = await screen.findByRole("button", { name: "Load more" });
  const folder = screen.getByRole("button", { name: "Alpha" });
  act(() => load.focus());
  fireEvent.keyDown(load, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(load, { key: "Enter", code: "Enter" });
  act(() => folder.focus());
  await act(async () => pending.resolve(page([row(2, "Beta")])));
  expect(load.isConnected).toBe(false);
  expect(document.activeElement).toBe(folder);
});

test("keyboard paging restores panel focus if the final page has no rows", async () => {
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(opened(1, [row(1, "Alpha")], "next")),
    page: vi.fn().mockResolvedValue(page([])),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const load = await screen.findByRole("button", { name: "Load more" });
  act(() => load.focus());
  await act(async () => {
    fireEvent.keyDown(load, { key: "Enter", code: "Enter" });
    fireEvent.keyUp(load, { key: "Enter", code: "Enter" });
  });
  expect(load.isConnected).toBe(false);
  expect(document.activeElement).toBe(
    screen.getByRole("region", { name: "Folder navigation" }),
  );
});
