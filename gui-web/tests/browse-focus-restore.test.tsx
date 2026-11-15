import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
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
 * empty folder and entering the screen. The review of PR #63 adds a pressed
 * "Up" that stays usable, a page retry, a final page without rows, an empty
 * folder without "Up" and closing item details whose row went away.
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

function pressPointer(target: HTMLElement) {
  act(() => target.focus());
  const pointer = { pointerType: "mouse", pointerId: 1, isPrimary: true };
  fireEvent.pointerDown(target, pointer);
  fireEvent.pointerUp(target, pointer);
  fireEvent.click(target, { detail: 1 });
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

test.each([
  ["keyboard", pressKey],
  ["pointer", pressPointer],
] as const)(
  "Up pressed by %s keeps the focus while it is still usable",
  async (_, press) => {
    const open = vi
      .fn()
      .mockResolvedValueOnce(folder(1, [1, 2, 3], [row(4, "Child")]))
      .mockImplementationOnce((request) =>
        Promise.resolve(
          folder(
            request.requestSeq,
            [1, 2],
            [row(3, "Parent A"), row(5, "Parent B")],
          ),
        ),
      );
    render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
    await screen.findByText("Child");
    const up = screen.getByRole("button", { name: "Up" }) as HTMLButtonElement;
    await act(async () => press(up));
    await screen.findByText("Parent A");
    expect(open).toHaveBeenCalledTimes(2);
    expect(up.disabled).toBe(false);
    expect(document.activeElement).toBe(up);
  },
);

test("Alt+Up from a row focuses the first row of the parent folder", async () => {
  const open = vi
    .fn()
    .mockResolvedValueOnce(folder(1, [1, 2, 3], [row(4, "Child")]))
    .mockImplementationOnce((request) =>
      Promise.resolve(
        folder(
          request.requestSeq,
          [1, 2],
          [row(3, "Parent A"), row(5, "Parent B")],
        ),
      ),
    );
  render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
  await screen.findByText("Child");
  const child = gridRow("Child");
  act(() => child.focus());
  await act(async () =>
    fireEvent.keyDown(child, { key: "ArrowUp", altKey: true }),
  );
  await screen.findByText("Parent A");
  expect(document.activeElement).toBe(gridRow("Parent A"));
});

test("Up that becomes disabled at the top still hands focus to the first row", async () => {
  // In Chromium a disabled control is never the active element: disabling
  // the focused "Up" moves focus to the body during the commit. jsdom keeps
  // it focused, so the test reads the active element the way Chromium does.
  let owner: object | null = document;
  while (owner && !Object.getOwnPropertyDescriptor(owner, "activeElement"))
    owner = Object.getPrototypeOf(owner) as object | null;
  const read = Object.getOwnPropertyDescriptor(owner!, "activeElement")!.get!;
  const active = vi
    .spyOn(document, "activeElement", "get")
    .mockImplementation(function (this: Document) {
      const element = read.call(this) as Element | null;
      return element instanceof HTMLButtonElement && element.disabled
        ? this.body
        : element;
    });
  try {
    const open = vi
      .fn()
      .mockResolvedValueOnce(folder(1, [1, 2], [row(3, "Child")]))
      .mockImplementationOnce((request) =>
        Promise.resolve(
          folder(request.requestSeq, [1], [row(2, "Top A"), row(5, "Top B")]),
        ),
      );
    render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
    await screen.findByText("Child");
    const up = screen.getByRole("button", { name: "Up" }) as HTMLButtonElement;
    await act(async () => pressKey(up));
    await screen.findByText("Top A");
    expect(up.disabled).toBe(true);
    expect(document.activeElement).toBe(gridRow("Top A"));
  } finally {
    active.mockRestore();
  }
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
  // The title is a focus fallback reachable only by script.
  expect(
    screen
      .getByRole("heading", { name: "Folders and files" })
      .getAttribute("tabindex"),
  ).toBe("-1");
});

test("an empty folder without an available Up focuses the title", async () => {
  // A directly opened scope root has no "Up".
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
  expect(document.activeElement).toBe(
    screen.getByRole("heading", { name: "Folders and files" }),
  );
  expect(document.activeElement).not.toBe(panel());
});

const entries = (count: number) =>
  Array.from({ length: count }, (_, index) =>
    row(index + 10, "Entry " + index),
  );

test("Retry page succeeding focuses the first row it loaded", async () => {
  const page_ = vi
    .fn()
    .mockRejectedValueOnce(new Error("unavailable"))
    .mockResolvedValueOnce(
      page([row(500, "Added 0"), row(501, "Added 1")], null, 202),
    );
  const open = vi
    .fn()
    .mockResolvedValue(folder(1, [1], entries(200), "next", 202));
  render(<BrowsePanel port={{ open, page: page_ }} resultVersion={id} />);
  const load = await screen.findByRole("button", { name: "Load more" });
  await act(async () => pressPointer(load));
  const retry = await screen.findByRole("button", { name: "Retry page" });
  expect(document.activeElement).toBe(retry);
  await act(async () => pressPointer(retry));
  expect(retry.isConnected).toBe(false);
  expect(document.activeElement).toBe(gridRow("Added 0"));
  expect(document.activeElement).not.toBe(panel());
});

test.each([
  ["keyboard", pressKey],
  ["pointer", pressPointer],
] as const)(
  "Load more by %s adding no rows focuses the last row and says so once",
  async (_, press) => {
    const open = vi
      .fn()
      .mockResolvedValue(
        folder(1, [1], [row(2, "A"), row(3, "B"), row(4, "C")], "next", 3),
      );
    const read = vi.fn().mockResolvedValue(page([], null, 3));
    render(<BrowsePanel port={{ open, page: read }} resultVersion={id} />);
    const load = await screen.findByRole("button", { name: "Load more" });
    await screen.findByText("C");
    await act(async () => press(load));
    expect(load.isConnected).toBe(false);
    expect(document.activeElement).toBe(gridRow("C"));
    const said = screen
      .getAllByRole("status")
      .filter((region) => region.textContent?.includes("No more items"));
    expect(said).toHaveLength(1);
    expect(said[0]!.textContent).toBe("No more items");
  },
);

test.each([
  ["the row at the same position", [1, 2], [row(2, "X"), row(5, "Y")], "Y"],
  ["the last row", [1, 2], [row(2, "X")], "X"],
  ["Up", [1, 2], [], "Up"],
  ["the title", [1], [], "title"],
] as const)(
  "closing details whose row went away focuses %s",
  async (_, parent, rows, expected) => {
    const open = vi
      .fn()
      .mockResolvedValueOnce(
        folder(1, [...parent, 7], [row(8, "Opener A"), row(9, "Opener B")]),
      )
      .mockImplementationOnce((request) =>
        Promise.resolve(folder(request.requestSeq, [...parent], [...rows])),
      );
    render(
      <BrowsePanel
        port={{ open, page: vi.fn() }}
        resultVersion={id}
        inspectItem={() => "Observed folder"}
      />,
    );
    await screen.findByText("Opener B");
    const opener = gridRow("Opener B");
    fireEvent.contextMenu(opener);
    const menu = await screen.findByRole("menu", {
      name: "Actions for Opener B",
    });
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Inspect" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Folder details",
    });
    // The folder changes under the dialog, taking the opener's row with it.
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Up", hidden: true })),
    );
    await waitFor(() => expect(opener.isConnected).toBe(false));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close details" }),
    );
    const target = () =>
      expected === "Up"
        ? screen.getByRole("button", { name: "Up" })
        : expected === "title"
          ? screen.getByRole("heading", { name: "Folders and files" })
          : gridRow(expected);
    await waitFor(() => expect(document.activeElement).toBe(target()));
    expect(document.activeElement).not.toBe(panel());
  },
);
