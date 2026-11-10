import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Action, BrowseGrid } from "../src/components";
import type { BrowseGridFocus } from "../src/components";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const count = 1_000_000;
const onMove = vi.fn();
const onOpen = vi.fn();
const onFocusRow = vi.fn();
const onVisible = vi.fn();

function grid(
  rowCount: number,
  focusRequest?: BrowseGridFocus,
  key = "query",
  total = count,
) {
  return (
    <BrowseGrid
      key={key}
      label="Folder items"
      columns={["Name", "Kind"]}
      count={total}
      rowCount={rowCount}
      busy={false}
      focusRequest={focusRequest}
      onMove={onMove}
      onOpen={onOpen}
      onFocusRow={onFocusRow}
      onVisible={onVisible}
      getRow={(index) => index}
      getKey={(index) => "row-" + index}
      renderCells={(index) => ["Item " + index, "file"]}
    />
  );
}

test.each([count + 1, -1])(
  "preserves absolute grid coordinates with aria-rowcount %s",
  async (rowCount) => {
    vi.stubGlobal("innerHeight", 480);
    let bodyTop = 0;
    const originalRect = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        return this.classList.contains("du-browse-grid__body")
          ? new DOMRect(0, bodyTop, 800, 8_000_000)
          : originalRect.call(this);
      },
    );
    render(grid(rowCount));
    const surface = await screen.findByRole("grid", { name: "Folder items" });
    expect(surface.getAttribute("aria-rowcount")).toBe(String(rowCount));
    expect(within(surface).getAllByRole("row").length).toBeLessThanOrEqual(29);
    bodyTop = -(8_000_000 - 480);
    fireEvent.scroll(window);
    const last = await within(surface).findByText("Item 999999");
    expect(last.closest('[role="row"]')?.getAttribute("aria-rowindex")).toBe(
      "1000001",
    );
    expect(within(surface).getAllByRole("row").length).toBeLessThanOrEqual(29);
  },
);

test("dispatches absolute navigation and toggles selection independently of focus", async () => {
  render(grid(count + 1));
  const first = (await screen.findByText("Item 0")).closest<HTMLElement>(
    '[role="row"]',
  )!;
  const next = screen.getByText("Item 1").closest<HTMLElement>('[role="row"]')!;
  act(() => first.focus());
  fireEvent.keyDown(first, { key: " ", code: "Space" });
  fireEvent.keyUp(first, { key: " ", code: "Space" });
  expect(first.getAttribute("aria-selected")).toBe("true");
  fireEvent.keyDown(first, { key: "ArrowDown" });
  expect(onMove).toHaveBeenLastCalledWith(1, false);
  fireEvent.keyDown(first, { key: "End" });
  expect(onMove).toHaveBeenLastCalledWith(999999, true);
  fireEvent.keyDown(first, { key: "Enter" });
  expect(onOpen).toHaveBeenLastCalledWith(0);
  act(() => next.focus());
  expect(first.getAttribute("aria-selected")).toBe("true");
  expect(next.getAttribute("aria-selected")).toBe("false");
});

test("mounts a requested distant row and rechecks focus eligibility at commit", async () => {
  const { rerender } = render(grid(count + 1));
  const first = (await screen.findByText("Item 0")).closest<HTMLElement>(
    '[role="row"]',
  )!;
  act(() => first.focus());
  const finish = vi.fn();
  rerender(
    grid(count + 1, { index: count - 1, eligible: () => false, finish }),
  );
  expect(finish).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(first);
  rerender(grid(count + 1, { index: count - 1, eligible: () => true, finish }));
  expect(finish).toHaveBeenCalledTimes(2);
  expect(document.activeElement?.getAttribute("aria-rowindex")).toBe("1000001");
});

function documentGeometry() {
  let scroll = 0;
  const sizes = { first: 64 };
  vi.stubGlobal("innerHeight", 480);
  vi.spyOn(window, "scrollY", "get").mockImplementation(() => scroll);
  const scrollBy = vi.spyOn(window, "scrollBy").mockImplementation((_, y) => {
    if (typeof y === "number") scroll += y;
  });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this.classList.contains("du-browse-grid__body"))
        return new DOMRect(0, -scroll, 800, 8_000_000);
      if (this.dataset.row !== undefined) {
        const top = Number.parseFloat(this.style.insetBlockStart) - scroll;
        return new DOMRect(
          0,
          top,
          800,
          this.dataset.row === "0" ? sizes.first : 64,
        );
      }
      return new DOMRect();
    },
  );
  return {
    sizes,
    scrollBy,
    scrollTo(top: number) {
      scroll = top;
      fireEvent.scroll(window);
    },
  };
}

function itemRow(index: number) {
  return screen
    .getByText("Item " + index)
    .closest<HTMLElement>('[role="row"]')!;
}

test("reveals the distant row inside the document viewport before giving it focus", async () => {
  const geometry = documentGeometry();
  const { rerender } = render(grid(count + 1));
  await screen.findByText("Item 0");
  const originalFocus = HTMLElement.prototype.focus;
  const seen: DOMRect[] = [];
  vi.spyOn(HTMLElement.prototype, "focus").mockImplementation(function (
    this: HTMLElement,
    options,
  ) {
    if (this.dataset.row === String(count - 1))
      seen.push(this.getBoundingClientRect());
    originalFocus.call(this, options);
  });
  const finish = vi.fn();
  rerender(grid(count + 1, { index: count - 1, eligible: () => true, finish }));
  expect(geometry.scrollBy).toHaveBeenCalledOnce();
  expect(seen.length).toBeGreaterThan(0);
  for (const frame of seen) {
    expect(frame.top).toBeGreaterThanOrEqual(0);
    expect(frame.bottom).toBeLessThanOrEqual(window.innerHeight);
  }
  expect(document.activeElement).toBe(itemRow(count - 1));
  expect(finish).toHaveBeenCalledOnce();
});

test("reveals an adjacent row when the logical move is smaller than one native scroll pixel", async () => {
  const geometry = documentGeometry();
  const total = 100_000_000;
  const { rerender } = render(grid(total + 1, undefined, "huge", total));
  await screen.findByText("Item 7");
  act(() => itemRow(7).focus());
  const finish = vi.fn();
  rerender(
    grid(total + 1, { index: 8, eligible: () => true, finish }, "huge", total),
  );
  const target = itemRow(8);
  expect(geometry.scrollBy).not.toHaveBeenCalled();
  expect(target.getBoundingClientRect().bottom).toBe(window.innerHeight);
  expect(document.activeElement).toBe(target);
  expect(finish).toHaveBeenCalledOnce();
  geometry.scrollTo(1);
  expect(target.getBoundingClientRect().bottom).toBeLessThanOrEqual(0);
  expect(document.activeElement).toBe(target);
});

test("rechecks eligibility after scrolling and before the positioned row receives focus", async () => {
  const geometry = documentGeometry();
  const { rerender } = render(grid(count + 1));
  await screen.findByText("Item 0");
  const initial = itemRow(0);
  act(() => initial.focus());
  let uninterrupted = true;
  const move = geometry.scrollBy.getMockImplementation()!;
  geometry.scrollBy.mockImplementation((x, y) => {
    move(x, y);
    uninterrupted = false;
  });
  const finish = vi.fn();
  rerender(
    grid(count + 1, {
      index: count - 1,
      eligible: () => uninterrupted,
      finish,
    }),
  );
  expect(geometry.scrollBy).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(initial);
  expect(finish).toHaveBeenCalledOnce();
});

test("keeps focus pinned, releases it on blur, and retains selection when the row remounts", async () => {
  const geometry = documentGeometry();
  render(
    <>
      {grid(count + 1)}
      <button>Outside grid</button>
    </>,
  );
  await screen.findByText("Item 0");
  const first = itemRow(0);
  act(() => first.focus());
  fireEvent.keyDown(first, { key: " " });
  expect(first.getAttribute("aria-selected")).toBe("true");
  geometry.scrollTo(2_000);
  expect(itemRow(0)).toBe(first);
  expect(document.activeElement).toBe(first);
  expect(
    screen.getByRole("grid").querySelectorAll("[data-row]").length,
  ).toBeLessThanOrEqual(29);
  act(() => screen.getByRole("button", { name: "Outside grid" }).focus());
  expect(screen.queryByText("Item 0")).toBeNull();
  geometry.scrollTo(0);
  expect(itemRow(0)).not.toBe(first);
  expect(itemRow(0).getAttribute("aria-selected")).toBe("true");
});

test("leaves guarded keys to their targets and keeps a single grid Tab stop", async () => {
  const inputKey = vi.fn();
  const move = vi.fn();
  render(
    <>
      <button>Before</button>
      <BrowseGrid<number>
        label="Folder items"
        columns={["Name", "Kind"]}
        count={2}
        rowCount={3}
        busy={false}
        focusRequest={undefined}
        onMove={move}
        onOpen={onOpen}
        onFocusRow={onFocusRow}
        onVisible={onVisible}
        getRow={(index) => index}
        getKey={String}
        renderCells={(index) => [
          <Action excludeFromTabOrder onPress={() => undefined}>
            Folder {index}
          </Action>,
          <input
            aria-label={"Field " + index}
            tabIndex={-1}
            onKeyDown={inputKey}
          />,
        ]}
      />
      <button>After</button>
      <div role="dialog" aria-label="Guard" hidden />
    </>,
  );
  const grid = await screen.findByRole("grid");
  const first = grid.querySelector<HTMLElement>('[data-row="0"]')!;
  act(() => first.focus());
  const field = screen.getByRole("textbox", { name: "Field 0" });
  expect(fireEvent.keyDown(field, { key: "End" })).toBe(true);
  expect(inputKey).toHaveBeenCalledOnce();
  for (const extra of [
    { isComposing: true },
    { keyCode: 229 },
    { shiftKey: true },
    { ctrlKey: true },
  ]) {
    expect(fireEvent.keyDown(first, { key: "End", ...extra })).toBe(true);
    expect(document.activeElement).toBe(first);
  }
  const dialog = screen.getByRole("dialog", { hidden: true });
  dialog.removeAttribute("hidden");
  expect(fireEvent.keyDown(first, { key: "End" })).toBe(true);
  expect(move).not.toHaveBeenCalled();
  dialog.setAttribute("hidden", "");
  expect(grid.querySelectorAll('[tabindex="0"]')).toHaveLength(1);
  expect(screen.getByRole("button", { name: "Folder 1" }).tabIndex).toBe(-1);
  expect(fireEvent.keyDown(first, { key: "Tab" })).toBe(true);
  expect(fireEvent.keyDown(first, { key: "Tab", shiftKey: true })).toBe(true);
  const after = screen.getByRole("button", { name: "After" });
  expect(fireEvent.keyDown(after, { key: "End", ctrlKey: true })).toBe(true);
});
