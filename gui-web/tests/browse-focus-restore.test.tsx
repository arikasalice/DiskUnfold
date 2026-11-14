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

/*
 * F02 FR8.HD1, design review of PR #60 (follow-up 3): focus never rests on the
 * folder panel; it goes to the nearest next step. The reviewed cases are a
 * folder opened by keyboard, "Load more" going away, "Retry" succeeding, an
 * empty folder and entering the screen. Steps the review does not name keep
 * the panel as their fallback.
 */

afterEach(cleanup);

const id = "018f0000-0000-7000-8000-0000000000f1";
const ref = (itemNo: number) => ({ resultVersion: id, itemNo });
const unknown = { state: "unknown", reasonKey: "ref.unavailable" } as const;

function row(itemNo: number, name: string): FolderQueryRow {
  return {
    item: ref(itemNo),
    displayName: { state: "present", value: name },
    kind: "directory",
    logicalSize: unknown,
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: unknown,
    flags: {
      unenumerated: { state: "absent" },
      excluded: { state: "absent" },
    },
    correspondence: unknown,
  };
}

function page(
  rows: FolderQueryRow[],
  nextCursor: string | null = null,
  count = rows.length,
): FolderQueryPage {
  return {
    rowKind: "folder",
    queryRef: id,
    resultVersion: id,
    rows,
    total: { state: "exact", count },
    unknownCount: 0,
    observedAt: null,
    completeness: "complete",
    nextCursor,
  };
}

/** A folder at `path` (scope root first) holding `rows`. */
function folder(
  requestSeq: number,
  path: number[],
  rows: FolderQueryRow[],
  nextCursor: string | null = null,
  count = rows.length,
): BrowseOpenResponse {
  return {
    kind: "opened",
    requestSeq,
    origin: "scan",
    location: path.length ? ref(path.at(-1)!) : null,
    breadcrumbs: path.map((itemNo) => ({
      item: ref(itemNo),
      displayName: { state: "present", value: `Crumb ${itemNo}` },
    })),
    rootKind:
      path.length > 1 ? "folder" : path.length ? "scope_root" : "virtual_root",
    currentCorrespondence: false,
    page: page(rows, nextCursor, count),
    sortStrategies: [],
    predicateStrategies: [],
  };
}

const gridRow = (name: string) =>
  screen.getByText(name).closest<HTMLElement>('[role="row"]')!;
const panel = () => screen.getByRole("region", { name: "Folder navigation" });

function pressKey(target: HTMLElement) {
  act(() => target.focus());
  fireEvent.keyDown(target, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(target, { key: "Enter", code: "Enter" });
}

test("a folder opened by keyboard focuses the first row of the new folder", async () => {
  const open = vi
    .fn()
    .mockResolvedValueOnce(folder(1, [1], [row(2, "Projects")]))
    .mockImplementationOnce((request) =>
      Promise.resolve(
        folder(request.requestSeq, [1, 2], [row(3, "Alpha"), row(4, "Beta")]),
      ),
    );
  render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
  await screen.findByText("Projects");
  pressKey(gridRow("Projects"));
  await screen.findByText("Alpha");
  expect(document.activeElement).toBe(gridRow("Alpha"));
  expect(document.activeElement).not.toBe(panel());
});

test("Load more going away focuses the first row it loaded", async () => {
  const first = Array.from({ length: 200 }, (_, index) =>
    row(index + 10, `Entry ${index}`),
  );
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(folder(1, [1], first, "next", 202)),
    page: vi
      .fn()
      .mockResolvedValue(
        page([row(500, "Added 0"), row(501, "Added 1")], null, 202),
      ),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const load = await screen.findByRole("button", { name: "Load more" });
  act(() => load.focus());
  const pointer = { pointerType: "mouse", pointerId: 1, isPrimary: true };
  await act(async () => {
    fireEvent.pointerDown(load, pointer);
    fireEvent.pointerUp(load, pointer);
    fireEvent.click(load, { detail: 1 });
  });
  expect(load.isConnected).toBe(false);
  expect(document.activeElement).toBe(gridRow("Added 0"));
});

test("Retry succeeding and going away focuses the first row of the list", async () => {
  const open = vi
    .fn()
    .mockRejectedValueOnce(new Error("unavailable"))
    .mockImplementationOnce((request) =>
      Promise.resolve(
        folder(request.requestSeq, [], [row(1, "First"), row(2, "Second")]),
      ),
    );
  render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
  const retry = await screen.findByRole("button", { name: "Retry" });
  await act(async () => pressKey(retry));
  expect(retry.isConnected).toBe(false);
  expect(document.activeElement).toBe(gridRow("First"));
});

test("an empty folder focuses Up", async () => {
  const open = vi
    .fn()
    .mockResolvedValueOnce(folder(1, [1, 2], [row(3, "Empty")]))
    .mockImplementationOnce((request) =>
      Promise.resolve(folder(request.requestSeq, [1, 2, 3], [])),
    );
  render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
  await screen.findByText("Empty");
  pressKey(gridRow("Empty"));
  await screen.findByText("No observed items in this folder.");
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Up" }),
  );
});

test("entering the screen focuses the title, which keeps it while the list opens", async () => {
  const open = vi.fn().mockResolvedValue(folder(1, [], [row(1, "First")]));
  render(
    <BrowsePanel
      port={{ open, page: vi.fn() }}
      resultVersion={id}
      focusTitle
    />,
  );
  const title = screen.getByRole("heading", { name: "Folders and files" });
  expect(document.activeElement).toBe(title);
  expect(title.getAttribute("tabindex")).toBe("-1");
  await screen.findByText("First");
  expect(document.activeElement).toBe(title);
});

test("a panel embedded without focusTitle leaves focus where it was", async () => {
  const open = vi.fn().mockResolvedValue(folder(1, [], [row(1, "First")]));
  render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
  await screen.findByText("First");
  expect(document.activeElement).toBe(document.body);
  expect(
    screen
      .getByRole("heading", { name: "Folders and files" })
      .hasAttribute("tabindex"),
  ).toBe(false);
});

test("an empty folder without an available Up keeps the panel as the fallback", async () => {
  // Not a reviewed case: a directly opened scope root has no "Up".
  const open = vi
    .fn()
    .mockRejectedValueOnce(new Error("unavailable"))
    .mockImplementationOnce((request) =>
      Promise.resolve(folder(request.requestSeq, [1], [])),
    );
  render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
  const retry = await screen.findByRole("button", { name: "Retry" });
  await act(async () => pressKey(retry));
  await screen.findByText("No observed items in this folder.");
  const up = screen.getByRole("button", { name: "Up" }) as HTMLButtonElement;
  expect(up.disabled).toBe(true);
  expect(document.activeElement).toBe(panel());
});
