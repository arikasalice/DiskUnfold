import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import axe from "axe-core";
import { afterEach, expect, test, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import { AnalysisError } from "../src/ipc";
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
  count = rows.length,
  state: FolderQueryPage["total"]["state"] = "exact",
): FolderQueryPage => ({
  rowKind: "folder",
  queryRef: id,
  resultVersion: id,
  rows,
  total: { state, count },
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
function openingPage(
  first: FolderQueryRow[],
  count = 201,
  state: FolderQueryPage["total"]["state"] = "exact",
  cursor = "next",
) {
  const rows = [...first];
  while (rows.length < Math.min(200, count)) {
    const index = rows.length;
    rows.push({ ...row(index + 1, "Entry " + index), kind: "file" });
  }
  const result = opened(1, rows, cursor);
  if (result.kind !== "opened") throw new Error("wrong fixture");
  result.page = page(rows, cursor, count, state);
  return result;
}

function gridRow(name: string) {
  return screen.getByText(name).closest<HTMLElement>('[role="row"]')!;
}

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
  const first = openingPage([row(1, "Alpha")], 201, "lower_bound", "token");
  first.page.completeness = "partial";
  await act(async () => initial.resolve(first));
  expect(screen.getByText("Scope roots")).toBeDefined();
  expect(screen.getByText(/Results are incomplete/)).toBeDefined();
  expect(screen.getByRole("grid").getAttribute("aria-rowcount")).toBe("-1");
  const load = screen.getByRole("button", { name: "Load more" });
  act(() => load.focus());
  pressLoadMore(load);
  expect(port.page).toHaveBeenCalledWith({
    queryRef: id,
    resultVersion: id,
    start: 200,
    count: 200,
  });
  await act(async () =>
    later.resolve(page([row(201, "Beta")], null, 201, "lower_bound")),
  );
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
  const path = screen.getByRole("navigation", { name: "Folder path" });
  expect(path.tagName).toBe("NAV");
  expect(
    within(path).getByText("Scope roots").getAttribute("aria-current"),
  ).toBe("location");
  expect(within(path).queryByRole("button", { name: "Up" })).toBeNull();
  alpha.focus();
  fireEvent.keyDown(alpha, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(alpha, { key: "Enter", code: "Enter" });
  expect(
    await screen.findByRole("button", { name: "Scope roots" }),
  ).toBeDefined();
  const current = within(path).getByText("Alpha");
  expect(current.getAttribute("aria-current")).toBe("location");
  expect(path.querySelectorAll("[aria-current]")).toHaveLength(1);
  expect(within(path).queryByRole("button", { name: "Alpha" })).toBeNull();
  const accessibility = await axe.run(path, {
    runOnly: {
      type: "rule",
      values: [
        "aria-valid-attr",
        "aria-valid-attr-value",
        "button-name",
        "list",
        "listitem",
      ],
    },
  });
  expect(accessibility.violations).toEqual([]);
  fireEvent.click(current);
  expect(port.open).toHaveBeenCalledTimes(2);
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

test("Enter opens pinned file details and closing returns focus to the file", async () => {
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValue(
        opened(1, [{ ...row(1, "observed.bin"), kind: "file" }]),
      ),
    page: vi.fn(),
  };
  const inspectFile = vi.fn(() => null);
  render(
    <BrowsePanel port={port} resultVersion={id} inspectFile={inspectFile} />,
  );
  const file = await screen.findByRole("button", { name: "observed.bin" });
  const fileRow = file.closest<HTMLElement>('[role="row"]')!;
  act(() => fileRow.focus());
  fireEvent.keyDown(fileRow, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(fileRow, { key: "Enter", code: "Enter" });
  const dialog = await screen.findByRole("dialog", {
    name: "File details — observed.bin",
  });
  expect(inspectFile).toHaveBeenCalledWith(ref(1));
  expect(port.open).toHaveBeenCalledTimes(1);
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Close details" }),
  );
  await act(async () => {});
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(fileRow);
});

test("rejects a page from another result without discarding visible rows", async () => {
  const bad = page([row(2, "Wrong")]);
  bad.resultVersion = "other";
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(openingPage([row(1, "Alpha")])),
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
      .mockResolvedValueOnce(openingPage([row(1, "Alpha")]))
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
      pending.resolve(page([{ ...row(201, "New item"), kind }], null, 201)),
    );
    expect(load.isConnected).toBe(false);
    expect(document.activeElement).toBe(gridRow("New item"));
    expect(gridRow("New item").getAttribute("aria-rowindex")).toBe("202");
    expect(screen.queryByText("Existing 199")).toBeNull();
  },
);

test("keyboard paging leaves focus where the user moved it before the final response", async () => {
  const pending = deferred<FolderQueryPage>();
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(openingPage([row(1, "Alpha")])),
    page: vi.fn(() => pending.promise),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const load = await screen.findByRole("button", { name: "Load more" });
  const folder = gridRow("Alpha");
  act(() => load.focus());
  fireEvent.keyDown(load, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(load, { key: "Enter", code: "Enter" });
  act(() => folder.focus());
  await act(async () => pending.resolve(page([row(201, "Beta")], null, 201)));
  expect(load.isConnected).toBe(false);
  expect(document.activeElement).toBe(folder);
});

test("keyboard paging restores panel focus if the final page has no rows", async () => {
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(openingPage([row(1, "Alpha")], 1)),
    page: vi.fn().mockResolvedValue(page([], null, 1)),
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
  await screen.findByRole("button", { name: "Child" });
  const child = gridRow("Child");
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
  await screen.findByRole("button", { name: "Child" });
  const child = gridRow("Child");
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
  expect(screen.getByText("Crumb 22").getAttribute("aria-current")).toBe(
    "location",
  );
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

test.each(["parent version", "current version", "current item"] as const)(
  "never makes an unverified breadcrumb current or actionable (%s)",
  async (mismatch) => {
    const response = openedAt(1, [11, 22], [row(33, "Child")]);
    const [parent, current] = response.breadcrumbs;
    if (!parent || !current) throw new Error("wrong fixture");
    if (mismatch === "parent version") parent.item.resultVersion = "other";
    else if (mismatch === "current version")
      current.item.resultVersion = "other";
    else current.item = ref(99);
    const port: BrowsePort = {
      open: vi.fn().mockResolvedValue(response),
      page: vi.fn(),
    };
    const { container } = render(
      <BrowsePanel port={port} resultVersion={id} />,
    );
    await screen.findByRole("button", { name: "Child" });
    const path = screen.getByRole("navigation", { name: "Folder path" });
    expect(within(path).getByText("Folder path unavailable")).toBeDefined();
    const marked = container.querySelectorAll("[aria-current]");
    if (mismatch === "parent version") {
      expect(marked).toHaveLength(1);
      expect(marked[0]?.textContent).toBe("Crumb 22");
    } else {
      expect(marked).toHaveLength(0);
    }
    expect(screen.queryByRole("button", { name: "Crumb 11" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Crumb 22" })).toBeNull();
    expect(port.open).toHaveBeenCalledTimes(1);
  },
);

test.each(["success", "moved focus", "failure"] as const)(
  "keeps the old breadcrumb while opening an ancestor and respects focus (%s)",
  async (outcome) => {
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
        <button>Elsewhere</button>
        <BrowsePanel port={port} resultVersion={id} />
      </>,
    );
    const path = await screen.findByRole("navigation", { name: "Folder path" });
    const ancestor = within(path).getByRole("button", { name: "Crumb 11" });
    const current = within(path).getByText("Crumb 22");
    ancestor.focus();
    fireEvent.keyDown(ancestor, { key: "Enter", code: "Enter" });
    fireEvent.keyUp(ancestor, { key: "Enter", code: "Enter" });
    expect(port.open).toHaveBeenLastCalledWith(
      expect.objectContaining({ location: ref(11), resultVersion: id }),
      expect.any(AbortSignal),
    );
    expect(within(path).getByText("Crumb 22")).toBe(current);
    expect(document.activeElement).toBe(ancestor);
    if (outcome === "failure") {
      await act(async () => pending.reject(new Error("unavailable")));
      expect(screen.getByRole("alert").textContent).toContain("Could not open");
      expect(within(path).getByText("Crumb 22")).toBe(current);
      expect(document.activeElement).toBe(ancestor);
      return;
    }
    const elsewhere = screen.getByRole("button", { name: "Elsewhere" });
    if (outcome === "moved focus") elsewhere.focus();
    await act(async () =>
      pending.resolve(openedAt(2, [11], [row(22, "Nested")])),
    );
    expect(ancestor.isConnected).toBe(false);
    expect(
      within(path).getByText("Crumb 11").getAttribute("aria-current"),
    ).toBe("location");
    expect(document.activeElement).toBe(
      outcome === "moved focus"
        ? elsewhere
        : screen.getByRole("region", { name: "Folder navigation" }),
    );
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
    2200,
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
  expect(document.activeElement).toBe(gridRow("Entry 200"));
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
      if (this.classList.contains("du-browse-grid__body"))
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

test.each([0, 1, 200])(
  "pointer terminal page (%i rows) restores focus without scrolling",
  async (count) => {
    const { pending, port } = delayedPage();
    const initial = count === 0 ? 200 : 200 + count;
    vi.mocked(port.open).mockResolvedValue(openingPage([], initial));
    const previous = window.scrollY;
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    try {
      window.scrollTo(0, 600);
      render(<BrowsePanel port={port} resultVersion={id} />);
      const load = await screen.findByRole("button", { name: "Load more" });
      act(() => load.focus());
      clickLoadMore(load);
      focus.mockClear();
      const rows = Array.from({ length: count }, (_, index) =>
        row(201 + index, "Added " + index),
      );
      await act(async () => pending.resolve(page(rows, null, initial)));
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
        if (this.classList.contains("du-browse-grid__body"))
          return DOMRect.fromRect({ y: -viewport });
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
        expect(document.activeElement).toBe(gridRow("Entry 200"));
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

test.each(["wheel", "touchstart", "scroll key", "pointer"] as const)(
  "cancels a pending focus intent on %s even if the viewport stays put",
  async (motion) => {
    const { pending, next, port } = delayedPage();
    render(<BrowsePanel port={port} resultVersion={id} />);
    const load = await screen.findByRole("button", { name: "Load more" });
    act(() => load.focus());
    pressLoadMore(load);
    if (motion === "scroll key") fireEvent.keyDown(load, { key: "PageUp" });
    else
      window.dispatchEvent(
        new Event(motion === "pointer" ? "pointerdown" : motion),
      );
    await act(async () => pending.resolve(next));
    expect(document.activeElement).toBe(load);
    expect(screen.queryByText("Entry 200")).toBeNull();
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
  expect(document.activeElement).toBe(gridRow("Alpha"));
  expect(screen.getByRole("button", { name: "Alpha" })).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(port.open).toHaveBeenLastCalledWith(
    expect.objectContaining({ location: ref(1), resultVersion: id }),
    expect.any(AbortSignal),
  );
  expect(await screen.findByRole("button", { name: "Retried" })).toBeDefined();
});

test.each(["first opening", "existing location"] as const)(
  "keeps keyboard Retry focused through another failure and hands off on success (%s)",
  async (context) => {
    const failingRetry = deferred<BrowseOpenResponse>();
    const successfulRetry = deferred<BrowseOpenResponse>();
    const open = vi.fn();
    if (context === "existing location")
      open.mockResolvedValueOnce(openedAt(1, [], [row(1, "Alpha")]));
    open
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockImplementationOnce(() => failingRetry.promise)
      .mockImplementationOnce(() => successfulRetry.promise);
    render(<BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />);
    if (context === "existing location")
      fireEvent.click(await screen.findByRole("button", { name: "Alpha" }));
    const retry = await screen.findByRole("button", { name: "Retry" });
    const error = screen.getByRole("alert");
    const path = screen.queryByRole("navigation", { name: "Folder path" });
    const attempts = open.mock.calls.length;
    retry.focus();
    act(() => {
      fireEvent.keyDown(retry, { key: "Enter", code: "Enter" });
      fireEvent.keyUp(retry, { key: "Enter", code: "Enter" });
      fireEvent.click(retry);
    });
    expect(open).toHaveBeenCalledTimes(attempts + 1);
    expect(screen.getByRole("button", { name: "Retrying…" })).toBe(retry);
    expect(retry.getAttribute("aria-disabled")).toBe("true");
    expect(document.activeElement).toBe(retry);
    expect(screen.getByRole("alert")).toBe(error);
    expect(screen.queryByRole("navigation", { name: "Folder path" })).toBe(
      path,
    );
    await act(async () => failingRetry.reject(new Error("still unavailable")));
    expect(screen.getByRole("button", { name: "Retry" })).toBe(retry);
    expect(document.activeElement).toBe(retry);

    fireEvent.keyDown(retry, { key: "Enter", code: "Enter" });
    fireEvent.keyUp(retry, { key: "Enter", code: "Enter" });
    expect(screen.getByRole("button", { name: "Retrying…" })).toBe(retry);
    expect(open.mock.lastCall?.[0]).toMatchObject({
      requestSeq: attempts + 2,
      resultVersion: id,
      ...(context === "existing location" ? { location: ref(1) } : {}),
    });
    await act(async () =>
      successfulRetry.resolve(
        openedAt(attempts + 2, context === "existing location" ? [1] : [], [
          row(2, "Retried"),
        ]),
      ),
    );
    expect(retry.isConnected).toBe(false);
    expect(screen.getByRole("button", { name: "Retried" })).toBeDefined();
    expect(document.activeElement).toBe(
      screen.getByRole("region", { name: "Folder navigation" }),
    );
  },
);

test("Retry success leaves focus where the user moved it while opening", async () => {
  const pending = deferred<BrowseOpenResponse>();
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockImplementationOnce(() => pending.promise),
    page: vi.fn(),
  };
  render(
    <>
      <button>Elsewhere</button>
      <BrowsePanel port={port} resultVersion={id} />
    </>,
  );
  const retry = await screen.findByRole("button", { name: "Retry" });
  retry.focus();
  fireEvent.click(retry);
  expect(screen.getByRole("button", { name: "Retrying…" })).toBe(retry);
  const elsewhere = screen.getByRole("button", { name: "Elsewhere" });
  elsewhere.focus();
  await act(async () => pending.resolve(opened(2, [row(1, "Retried")])));
  expect(document.activeElement).toBe(elsewhere);
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
  expect(document.activeElement).toBe(gridRow("Latest"));
});

function syntheticPage(
  count: number,
  start: number,
  state: FolderQueryPage["total"]["state"] = "exact",
) {
  const rows = Array.from(
    { length: Math.min(200, count - start) },
    (_, index) => ({
      ...row(start + index + 1, "Entry " + (start + index)),
      kind: "file" as const,
    }),
  );
  const cursor =
    start + rows.length < count ? "next-" + (start + rows.length) : null;
  return page(rows, cursor, count, state);
}

function syntheticPort(
  count: number,
  state: FolderQueryPage["total"]["state"] = "exact",
) {
  return {
    open: vi.fn<BrowsePort["open"]>(async () => ({
      ...openingPage([], count, state),
      page: syntheticPage(count, 0, state),
    })),
    page: vi.fn<BrowsePort["page"]>(async (request) => {
      if (!("start" in request)) throw new Error("Expected a position read");
      return syntheticPage(count, request.start, state);
    }),
  };
}

test.each([0, 1, 200, 201, 2001, 1_000_000])(
  "End focuses the exact registered end of %i items",
  async (count) => {
    const port = syntheticPort(count);
    render(<BrowsePanel port={port} resultVersion={id} />);
    const grid = await screen.findByRole("grid", { name: "Folder items" });
    const start = count === 0 ? grid : gridRow("Entry 0");
    act(() => start.focus());
    await act(async () => undefined);
    const callsBeforeEnd = port.page.mock.calls.length;
    await act(async () => fireEvent.keyDown(start, { key: "End" }));
    if (count === 0) {
      expect(document.activeElement).toBe(start);
    } else {
      expect(document.activeElement).toBe(gridRow("Entry " + (count - 1)));
      expect(document.activeElement?.getAttribute("aria-rowindex")).toBe(
        String(count + 1),
      );
    }
    expect(grid.getAttribute("aria-rowcount")).toBe(String(count + 1));
    const direct = port.page.mock.calls.slice(callsBeforeEnd);
    expect(direct).toEqual(
      count <= 201
        ? []
        : [
            [
              {
                queryRef: id,
                resultVersion: id,
                start: Math.floor((count - 1) / 200) * 200,
                count: 200,
              },
            ],
          ],
    );
    expect(port.open).toHaveBeenCalledTimes(1);
  },
);

test.each([0, 2001])(
  "End announces the unconfirmed list end for a lower bound of %i",
  async (count) => {
    const port = syntheticPort(count, "lower_bound");
    render(<BrowsePanel port={port} resultVersion={id} />);
    const grid = await screen.findByRole("grid", { name: "Folder items" });
    if (count > 0) await screen.findByText("Entry 0");
    const start = count === 0 ? grid : gridRow("Entry 0");
    act(() => start.focus());
    await act(async () => fireEvent.keyDown(start, { key: "End" }));
    expect(grid.getAttribute("aria-rowcount")).toBe("-1");
    expect(screen.getByText(/The end of the list is unknown/)).toBeDefined();
    expect(document.activeElement).toBe(
      count === 0 ? start : gridRow("Entry 2000"),
    );
    expect(
      port.page.mock.calls.filter(
        ([request]) => "start" in request && request.start === 2000,
      ),
    ).toHaveLength(count === 0 ? 0 : 1);
    expect(port.open).toHaveBeenCalledTimes(1);
  },
);

test.each([
  "failure",
  "version",
  "query",
  "focus away and back",
  "unmount",
] as const)(
  "a pending End read preserves the existing focus after %s",
  async (outcome) => {
    const count = 2001;
    const port = syntheticPort(count, "lower_bound");
    const pending = deferred<FolderQueryPage>();
    port.page.mockImplementation(async (request) =>
      "start" in request && request.start === 2000
        ? pending.promise
        : syntheticPage(count, 200, "lower_bound"),
    );
    const { unmount } = render(<BrowsePanel port={port} resultVersion={id} />);
    await screen.findByText("Entry 0");
    const start = gridRow("Entry 0");
    act(() => start.focus());
    await act(async () => fireEvent.keyDown(start, { key: "End" }));
    expect(screen.getByText("Loading folder items…")).toBeDefined();
    expect(document.activeElement).toBe(start);
    if (outcome === "focus away and back") {
      act(() => gridRow("Entry 1").focus());
      act(() => start.focus());
    }
    if (outcome === "unmount") unmount();
    await act(async () => {
      const result = syntheticPage(count, 2000, "lower_bound");
      if (outcome === "failure") pending.reject(new Error("no page"));
      else {
        if (outcome === "version") result.resultVersion = "another version";
        if (outcome === "query") result.queryRef = "another query";
        pending.resolve(result);
      }
    });
    expect(document.activeElement).toBe(
      outcome === "unmount" ? document.body : start,
    );
    expect(screen.queryByText("Entry 2000")).toBeNull();
    expect(port.open).toHaveBeenCalledTimes(1);
  },
);

test("the latest request alone moves focus when End and ArrowDown resolve out of order", async () => {
  const port = syntheticPort(2001);
  const final = deferred<FolderQueryPage>();
  const adjacent = deferred<FolderQueryPage>();
  port.page.mockImplementation(async (request) =>
    "start" in request && request.start === 2000
      ? final.promise
      : adjacent.promise,
  );
  const original = window.scrollY;
  const rect = vi
    .spyOn(HTMLElement.prototype, "getBoundingClientRect")
    .mockImplementation(function (this: HTMLElement) {
      return this.classList.contains("du-browse-grid__body")
        ? DOMRect.fromRect({ y: -window.scrollY })
        : DOMRect.fromRect({ width: 800 });
    });
  try {
    window.scrollTo(0, 64 * 191);
    render(<BrowsePanel port={port} resultVersion={id} />);
    await screen.findByText("Entry 199");
    const current = gridRow("Entry 199");
    act(() => current.focus());
    await act(async () => fireEvent.keyDown(current, { key: "End" }));
    await act(async () => fireEvent.keyDown(current, { key: "ArrowDown" }));
    await act(async () => final.resolve(syntheticPage(2001, 2000)));
    expect(document.activeElement).toBe(current);
    expect(screen.getByText("Loading folder items…")).toBeDefined();
    await act(async () => adjacent.resolve(syntheticPage(2001, 200)));
    expect(document.activeElement).toBe(gridRow("Entry 200"));
    expect(
      port.page.mock.calls.filter(
        ([request]) => "start" in request && request.start === 200,
      ),
    ).toHaveLength(1);
  } finally {
    cleanup();
    rect.mockRestore();
    window.scrollTo(0, original);
  }
});

test("page Retry keeps the same focused control across a repeated failure", async () => {
  const port = syntheticPort(2001);
  const firstRetry = deferred<FolderQueryPage>();
  let attempts = 0;
  port.page.mockImplementation(async (request) => {
    if (!("start" in request)) throw new Error("Expected a position read");
    if (request.start !== 2000) return syntheticPage(2001, request.start);
    attempts += 1;
    if (attempts === 1) throw new Error("initial page failure");
    if (attempts === 2) return firstRetry.promise;
    return syntheticPage(2001, 2000);
  });
  render(<BrowsePanel port={port} resultVersion={id} />);
  await screen.findByText("Entry 0");
  const initial = gridRow("Entry 0");
  act(() => initial.focus());
  await act(async () => fireEvent.keyDown(initial, { key: "End" }));
  expect(document.activeElement).toBe(initial);
  const retry = screen.getByRole("button", { name: "Retry page" });
  act(() => retry.focus());
  await act(async () => pressLoadMore(retry));
  expect(document.activeElement).toBe(retry);
  expect(screen.getByRole("button", { name: "Retrying page…" })).toBe(retry);
  await act(async () => pressLoadMore(retry));
  expect(attempts).toBe(2);
  await act(async () => firstRetry.reject(new Error("again")));
  expect(document.activeElement).toBe(retry);
  expect(screen.getByRole("button", { name: "Retry page" })).toBe(retry);
  await act(async () => pressLoadMore(retry));
  expect(document.activeElement).toBe(gridRow("Entry 2000"));
  expect(screen.queryByRole("button", { name: "Retry page" })).toBeNull();
});

test("stale position pages offer reopening the same folder and pinned result", async () => {
  const port = syntheticPort(2001);
  port.page.mockRejectedValue(
    new AnalysisError("STALE_VERSION", "query.stale"),
  );
  render(<BrowsePanel port={port} resultVersion={id} />);
  const reopen = await screen.findByRole("button", { name: "Reopen folder" });
  const first = gridRow("Entry 0");
  expect(first).toBeDefined();
  fireEvent.click(reopen);
  expect(port.open).toHaveBeenLastCalledWith(
    expect.objectContaining({ resultVersion: id, requestSeq: 2 }),
    expect.any(AbortSignal),
  );
});

test("folder activation restores the panel when the active button normalizes to its grid row", async () => {
  const pending = deferred<BrowseOpenResponse>();
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(opened(1, [row(1, "Alpha")]))
      .mockImplementationOnce(() => pending.promise),
    page: vi.fn(),
  };
  render(<BrowsePanel port={port} resultVersion={id} />);
  const alpha = await screen.findByRole("button", { name: "Alpha" });
  act(() => {
    alpha.focus();
    fireEvent.click(alpha);
  });
  expect(document.activeElement).toBe(gridRow("Alpha"));
  await act(async () => pending.resolve(opened(2, [row(2, "New location")])));
  expect(document.activeElement).toBe(
    screen.getByRole("region", { name: "Folder navigation" }),
  );
});

test("assistive-technology-style activation of Load more focuses the first new file", async () => {
  const { pending, next, port } = delayedPage();
  render(<BrowsePanel port={port} resultVersion={id} />);
  const load = await screen.findByRole("button", { name: "Load more" });
  act(() => load.focus());
  fireEvent.click(load, { detail: 0 });
  await act(async () => pending.resolve(next));
  expect(document.activeElement).toBe(gridRow("Entry 200"));
});

test("terminal pointer paging does not revive its focus intent after the user leaves and returns", async () => {
  const pending = deferred<FolderQueryPage>();
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(openingPage([row(1, "Alpha")], 1)),
    page: vi.fn(() => pending.promise),
  };
  render(
    <>
      <BrowsePanel port={port} resultVersion={id} />
      <button>Outside browse</button>
    </>,
  );
  const load = await screen.findByRole("button", { name: "Load more" });
  act(() => load.focus());
  clickLoadMore(load);
  act(() => screen.getByRole("button", { name: "Outside browse" }).focus());
  act(() => load.focus());
  await act(async () => pending.resolve(page([], null, 1)));
  expect(load.isConnected).toBe(false);
  expect(document.activeElement).toBe(document.body);
});

test("page Retry does not restore row or panel focus after the user leaves and returns", async () => {
  const port = syntheticPort(2001);
  const pending = deferred<FolderQueryPage>();
  let attempts = 0;
  port.page.mockImplementation(async (request) => {
    if (!("start" in request)) throw new Error("Expected a position read");
    if (request.start !== 2000) return syntheticPage(2001, request.start);
    if (++attempts === 1) throw new Error("page failure");
    return pending.promise;
  });
  render(
    <>
      <BrowsePanel port={port} resultVersion={id} />
      <button>Outside browse</button>
    </>,
  );
  await screen.findByText("Entry 0");
  const first = gridRow("Entry 0");
  act(() => first.focus());
  await act(async () => fireEvent.keyDown(first, { key: "End" }));
  const retry = screen.getByRole("button", { name: "Retry page" });
  act(() => retry.focus());
  pressLoadMore(retry);
  act(() => screen.getByRole("button", { name: "Outside browse" }).focus());
  act(() => retry.focus());
  await act(async () => pending.resolve(syntheticPage(2001, 2000)));
  expect(retry.isConnected).toBe(false);
  expect(document.activeElement).toBe(document.body);
});

test("the sparse grid exposes valid row, header and selection semantics", async () => {
  const port = syntheticPort(2001, "lower_bound");
  render(<BrowsePanel port={port} resultVersion={id} />);
  const grid = await screen.findByRole("grid", { name: "Folder items" });
  expect(
    within(grid)
      .getAllByRole("columnheader")
      .map((header) => header.textContent),
  ).toEqual([
    "Name",
    "Type",
    "Logical size",
    "Allocated size",
    "Modified",
    "Availability",
  ]);
  const first = gridRow("Entry 0");
  act(() => first.focus());
  fireEvent.keyDown(first, { key: " " });
  expect(first.getAttribute("aria-selected")).toBe("true");
  expect(first.querySelectorAll("[aria-colindex]")).toHaveLength(6);
  expect(first.querySelectorAll('span[aria-hidden="true"]')).toHaveLength(6);
  const result = await axe.run(grid, {
    runOnly: {
      type: "rule",
      values: [
        "aria-required-children",
        "aria-required-parent",
        "aria-roles",
        "aria-valid-attr",
        "aria-valid-attr-value",
        "nested-interactive",
      ],
    },
  });
  expect(result.violations).toEqual([]);
});
