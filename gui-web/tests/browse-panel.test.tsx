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

function openedAt(sequence: number, path: number[], rows: FolderQueryRow[]) {
  const response = opened(sequence, rows);
  if (response.kind !== "opened") throw new Error("wrong fixture");
  response.rootKind =
    path.length > 1 ? "folder" : path.length ? "scope_root" : "virtual_root";
  response.location = path.length ? ref(path.at(-1) ?? 0) : null;
  response.breadcrumbs = path.map((itemNo) => ({
    item: ref(itemNo),
    displayName: { state: "present", value: "Crumb " + itemNo },
  }));
  return response;
}

test("uses the pinned immediate parent for both Alt+Up and the Up button", async () => {
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(openedAt(1, [], [row(11, "Scope")]))
      .mockResolvedValueOnce(openedAt(2, [11], [row(22, "Nested")]))
      .mockResolvedValueOnce(openedAt(3, [11, 22], [row(33, "Child")]))
      .mockResolvedValueOnce(openedAt(4, [11], [row(22, "Nested")]))
      .mockResolvedValueOnce(openedAt(5, [], [row(11, "Scope")])),
    page: vi.fn(),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const scope = await screen.findByRole("button", { name: "Scope" });
  const up = screen.getByRole<HTMLButtonElement>("button", { name: "Up" });
  expect(up.disabled).toBe(true);
  expect(fireEvent.keyDown(scope, { key: "ArrowUp", altKey: true })).toBe(true);
  fireEvent.click(scope);
  fireEvent.click(await screen.findByRole("button", { name: "Nested" }));
  const child = await screen.findByRole("button", { name: "Child" });
  act(() => child.focus());
  expect(fireEvent.keyDown(child, { key: "ArrowUp", altKey: true })).toBe(
    false,
  );
  expect(port.open).toHaveBeenLastCalledWith(
    expect.objectContaining({
      resultVersion: id,
      location: ref(11),
      requestSeq: 4,
    }),
    expect.any(AbortSignal),
  );
  await screen.findByRole("button", { name: "Nested" });
  fireEvent.click(up);
  expect(port.open).toHaveBeenLastCalledWith(
    expect.not.objectContaining({ location: expect.anything() }),
    expect.any(AbortSignal),
  );
  expect(port.open).toHaveBeenLastCalledWith(
    expect.objectContaining({ resultVersion: id, requestSeq: 5 }),
    expect.any(AbortSignal),
  );
  await screen.findByRole("button", { name: "Scope" });
  expect(up.disabled).toBe(true);
});

test("keeps location and focus through an unsuccessful Up and suppresses duplicate or reserved keys", async () => {
  const pending = deferred<BrowseOpenResponse>();
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(openedAt(1, [11, 22], [row(33, "Child")]))
      .mockImplementationOnce(() => pending.promise),
    page: vi.fn(),
  };
  render(
    <>
      <BrowsePanel port={port} resultVersion={id} />
      <div style={{ display: "none" }}>
        <div role="listbox" aria-label="Popup" />
      </div>
    </>,
  );
  const child = await screen.findByRole("button", { name: "Child" });
  act(() => child.focus());
  const altUp = { key: "ArrowUp", altKey: true };
  for (const extra of [
    { isComposing: true },
    { keyCode: 229 },
    { shiftKey: true },
    { ctrlKey: true },
    { metaKey: true },
    { repeat: true },
  ]) {
    fireEvent.keyDown(child, { ...altUp, ...extra });
  }
  const popup = document.querySelector<HTMLElement>('[role="listbox"]');
  const popupParent = popup?.parentElement;
  if (!popupParent) throw new Error("wrong fixture");
  popupParent.style.display = "block";
  fireEvent.keyDown(child, altUp);
  popupParent.style.display = "none";
  const field = document.createElement("input");
  const panel = screen.getByRole("region", { name: "Folder navigation" });
  panel.append(field);
  fireEvent.keyDown(field, altUp);
  field.remove();
  expect(port.open).toHaveBeenCalledTimes(1);

  act(() => {
    expect(fireEvent.keyDown(child, altUp)).toBe(false);
    expect(fireEvent.keyDown(child, altUp)).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Up" }));
  });
  expect(port.open).toHaveBeenCalledTimes(2);
  expect(document.activeElement).toBe(child);
  await act(async () => pending.reject(new Error("unavailable")));
  expect(screen.getByRole("alert").textContent).toContain("Could not open");
  expect(screen.getByRole("button", { name: "Crumb 22" })).toBeDefined();
  expect(document.activeElement).toBe(child);
});

test("leaves Up disabled at a directly opened single scope root", async () => {
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(openedAt(1, [11], [row(33, "Child")])),
    page: vi.fn(),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const child = await screen.findByRole("button", { name: "Child" });
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Up" }).disabled,
  ).toBe(true);
  expect(fireEvent.keyDown(child, { key: "ArrowUp", altKey: true })).toBe(true);
  expect(port.open).toHaveBeenCalledTimes(1);
});

test.each(["parent version", "current crumb"] as const)(
  "does not derive Up from a mismatched %s",
  async (mismatch) => {
    const response = openedAt(1, [11, 22], [row(33, "Child")]);
    const [parent, current] = response.breadcrumbs;
    if (!parent || !current) throw new Error("wrong fixture");
    if (mismatch === "parent version") parent.item.resultVersion = "other";
    else current.item = ref(99);
    const port: BrowsePort = {
      open: vi.fn().mockResolvedValue(response),
      page: vi.fn(),
    };
    render(<BrowsePanel port={port} resultVersion={id} />);
    const child = await screen.findByRole("button", { name: "Child" });
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Up" }).disabled,
    ).toBe(true);
    expect(fireEvent.keyDown(child, { key: "ArrowUp", altKey: true })).toBe(
      true,
    );
    expect(port.open).toHaveBeenCalledTimes(1);
  },
);

function delayedPage() {
  const pending = deferred<FolderQueryPage>();
  const first = opened(
    1,
    Array.from({ length: 200 }, (_, index) => ({
      ...row(index + 1, "Entry " + index),
      kind: "file" as const,
    })),
    "next",
  );
  if (first.kind !== "opened") throw new Error("wrong fixture");
  first.page.total = { state: "exact", count: 2200 };
  const next = page(
    Array.from({ length: 200 }, (_, index) => ({
      ...row(index + 201, "Entry " + (index + 200)),
      kind: "file" as const,
    })),
    "more",
  );
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(first),
    page: vi.fn(() => pending.promise),
  };
  return { pending, next, port };
}

function pressLoadMore(button: HTMLElement) {
  fireEvent.keyDown(button, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(button, { key: "Enter", code: "Enter" });
}

test("keyboard paging continues at the first new row even when the button remains", async () => {
  const { pending, next, port } = delayedPage();
  render(<BrowsePanel port={port} resultVersion={id} />);
  const load = await screen.findByRole("button", { name: "Load more" });
  expect(screen.queryByText("Entry 200")).toBeNull();
  act(() => load.focus());
  pressLoadMore(load);
  await act(async () => pending.resolve(next));
  expect(screen.getByRole("button", { name: "Load more" })).toBe(load);
  expect(document.activeElement).toBe(
    screen.getByText("Entry 200").closest("tr"),
  );
  expect(screen.queryByText("Entry 399")).toBeNull();
});

test("scrolling while a keyboard page is pending prevents a focus or scroll jump", async () => {
  const { pending, next, port } = delayedPage();
  const previous = window.scrollY;
  const focus = vi.spyOn(HTMLElement.prototype, "focus");
  const scrollBy = vi.spyOn(window, "scrollBy");
  try {
    window.scrollTo(0, 12000);
    render(<BrowsePanel port={port} resultVersion={id} />);
    const load = await screen.findByRole("button", { name: "Load more" });
    act(() => load.focus());
    pressLoadMore(load);
    window.scrollTo(0, 600);
    fireEvent.scroll(window);
    const before = window.scrollY;
    expect(before).toBe(600);
    focus.mockClear();
    scrollBy.mockClear();
    await act(async () => pending.resolve(next));
    expect(window.scrollY).toBe(before);
    expect(document.activeElement).toBe(load);
    expect(focus).not.toHaveBeenCalled();
    expect(scrollBy).not.toHaveBeenCalled();
  } finally {
    focus.mockRestore();
    scrollBy.mockRestore();
    window.scrollTo(0, previous);
  }
});

test("pointer paging leaves the old viewport and focus alone when 200 rows are appended", async () => {
  const { pending, next, port } = delayedPage();
  const previous = window.scrollY;
  const rect = vi
    .spyOn(HTMLElement.prototype, "getBoundingClientRect")
    .mockImplementation(function (this: HTMLElement) {
      if (this.tagName === "TBODY")
        return DOMRect.fromRect({ y: -window.scrollY });
      if (this.hasAttribute("data-row"))
        return DOMRect.fromRect({ height: 64 });
      return DOMRect.fromRect({ width: 800 });
    });
  const scrollBy = vi.spyOn(window, "scrollBy");
  try {
    window.scrollTo(0, 64 * 195);
    render(<BrowsePanel port={port} resultVersion={id} />);
    const load = await screen.findByRole("button", { name: "Load more" });
    await screen.findByText("Entry 195");
    const before = window.scrollY;
    expect(before).toBe(64 * 195);
    act(() => load.focus());
    clickLoadMore(load);
    expect(port.page).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(next));
    expect(window.scrollY).toBe(before);
    expect(document.activeElement).toBe(load);
    expect(scrollBy).not.toHaveBeenCalled();
    expect(screen.getByText("Entry 195")).toBeDefined();
    expect(await screen.findByText("Entry 200")).toBeDefined();
    expect(screen.queryByText("Entry 399")).toBeNull();
  } finally {
    rect.mockRestore();
    scrollBy.mockRestore();
    window.scrollTo(0, previous);
  }
});

function clickLoadMore(button: HTMLElement) {
  const pointer = {
    pointerType: "mouse",
    pointerId: 1,
    isPrimary: true,
    button: 0,
  };
  fireEvent.pointerDown(button, pointer);
  fireEvent.pointerUp(button, pointer);
  fireEvent.click(button, { detail: 1 });
}

test.each([0, 1])(
  "pointer terminal page (%i rows) restores focus without scrolling",
  async (count) => {
    const { pending, port } = delayedPage();
    const previous = window.scrollY;
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    try {
      window.scrollTo(0, 600);
      render(<BrowsePanel port={port} resultVersion={id} />);
      const load = await screen.findByRole("button", { name: "Load more" });
      act(() => load.focus());
      clickLoadMore(load);
      focus.mockClear();
      const rows = count === 0 ? [] : [row(201, "Added")];
      await act(async () => pending.resolve(page(rows)));
      const panel = screen.getByRole("region", { name: "Folder navigation" });
      expect(load.isConnected).toBe(false);
      expect(window.scrollY).toBe(600);
      expect(document.activeElement).toBe(panel);
      expect(focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
    } finally {
      focus.mockRestore();
      window.scrollTo(0, previous);
    }
  },
);

test.each(["layout only", "user before layout", "user after layout"] as const)(
  "separates measured layout movement from unrelated scrolling while a page is pending (%s)",
  async (motion) => {
    const { pending, next, port } = delayedPage();
    let viewport = 64 * 4 + 8;
    let firstHeight = 64;
    let scheduled: FrameRequestCallback | undefined;
    const position = vi
      .spyOn(window, "scrollY", "get")
      .mockImplementation(() => viewport);
    const scrollBy = vi
      .spyOn(window, "scrollBy")
      .mockImplementation((_x, y) => {
        if (typeof y === "number") viewport += y;
      });
    const raf = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback) => {
        scheduled = callback;
        return 1;
      });
    const cancel = vi
      .spyOn(window, "cancelAnimationFrame")
      .mockImplementation(() => {
        scheduled = undefined;
      });
    const rect = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        if (this.tagName === "TBODY") return DOMRect.fromRect({ y: -viewport });
        if (this.hasAttribute("data-row")) {
          return DOMRect.fromRect({
            height: this.dataset.row === "0" ? firstHeight : 64,
          });
        }
        return DOMRect.fromRect({ width: 800 });
      });
    const flush = () =>
      act(() => {
        const callback = scheduled;
        scheduled = undefined;
        callback?.(0);
      });
    try {
      render(<BrowsePanel port={port} resultVersion={id} />);
      const load = await screen.findByRole("button", { name: "Load more" });
      flush();
      act(() => load.focus());
      pressLoadMore(load);

      // Remeasuring row 0 moves the ordinary anchor by 80 px.
      if (motion === "user before layout") viewport += 35;
      firstHeight += 80;
      fireEvent(window, new Event("resize"));
      flush();
      expect(scrollBy).toHaveBeenCalledExactlyOnceWith(0, 80);

      if (motion === "user after layout") viewport += 35;
      fireEvent.scroll(window);
      await act(async () => pending.resolve(next));
      if (motion === "layout only") {
        expect(document.activeElement).toBe(
          screen.getByText("Entry 200").closest("tr"),
        );
      } else {
        expect(document.activeElement).toBe(load);
        expect(screen.queryByText("Entry 200")).toBeNull();
      }
    } finally {
      cleanup();
      position.mockRestore();
      scrollBy.mockRestore();
      raf.mockRestore();
      cancel.mockRestore();
      rect.mockRestore();
    }
  },
);

test.each(["browser layout", "wheel", "scroll key", "pointer"] as const)(
  "distinguishes the browser's append commit scroll from intervening input (%s)",
  async (motion) => {
    const { pending, next, port } = delayedPage();
    let viewport = 12_264;
    let mounted = false;
    const scrollY = vi
      .spyOn(window, "scrollY", "get")
      .mockImplementation(() => viewport);
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe(node: HTMLElement) {
          if (mounted || node.dataset.row !== "200") return;
          mounted = true;
          // WebView can adjust scroll as the appended row enters the DOM, before a scroll event.
          viewport += 180;
          if (motion === "wheel")
            window.dispatchEvent(new WheelEvent("wheel", { deltaY: -120 }));
          if (motion === "scroll key")
            window.dispatchEvent(
              new KeyboardEvent("keydown", { key: "PageUp" }),
            );
          if (motion === "pointer")
            window.dispatchEvent(
              new PointerEvent("pointerdown", { pointerType: "mouse" }),
            );
        }
        unobserve() {}
        disconnect() {}
      },
    );
    try {
      render(<BrowsePanel port={port} resultVersion={id} />);
      const load = await screen.findByRole("button", { name: "Load more" });
      act(() => load.focus());
      pressLoadMore(load);
      await act(async () => pending.resolve(next));
      expect(mounted).toBe(true);
      expect(window.scrollY).toBe(12_444);
      if (motion === "browser layout") {
        expect(document.activeElement).toBe(
          screen.getByText("Entry 200").closest("tr"),
        );
      } else {
        expect(document.activeElement).toBe(load);
      }
    } finally {
      cleanup();
      scrollY.mockRestore();
      vi.unstubAllGlobals();
    }
  },
);

test("keeps the previous result and focus on failure, and retries the pinned location", async () => {
  const pending = deferred<BrowseOpenResponse>();
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(opened(1, [row(1, "Alpha")]))
      .mockImplementationOnce(() => pending.promise)
      .mockResolvedValueOnce(opened(3, [row(2, "Retried")])),
    page: vi.fn(),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const alpha = await screen.findByRole("button", { name: "Alpha" });
  act(() => alpha.focus());
  fireEvent.click(alpha);
  expect(screen.getByLabelText("Opening folder")).toBeDefined();
  expect(screen.getByRole("button", { name: "Alpha" })).toBeDefined();
  await act(async () => pending.reject(new Error("failed")));
  expect(document.activeElement).toBe(alpha);
  expect(screen.getByRole("button", { name: "Alpha" })).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(port.open).toHaveBeenLastCalledWith(
    expect.objectContaining({ location: ref(1), resultVersion: id }),
    expect.any(AbortSignal),
  );
  expect(await screen.findByRole("button", { name: "Retried" })).toBeDefined();
});

test("a stale failure cannot change the latest result or focus", async () => {
  const stale = deferred<BrowseOpenResponse>();
  const latest = deferred<BrowseOpenResponse>();
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(opened(1, [row(1, "Alpha"), row(2, "Beta")]))
      .mockImplementationOnce(() => stale.promise)
      .mockImplementationOnce(() => latest.promise),
    page: vi.fn(),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  fireEvent.click(await screen.findByRole("button", { name: "Alpha" }));
  expect(screen.getByRole("button", { name: "Alpha" })).toBeDefined();
  expect(screen.getByLabelText("Opening folder")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Beta" }));
  await act(async () => latest.resolve(opened(3, [row(3, "Latest")])));
  const latestRow = screen.getByRole("button", { name: "Latest" });
  act(() => latestRow.focus());
  await act(async () => stale.reject(new Error("late failure")));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("button", { name: "Latest" })).toBe(latestRow);
  expect(document.activeElement).toBe(latestRow);
});
