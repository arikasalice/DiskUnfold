import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { BrowseGrid } from "../src/components";
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

function grid(rowCount: number, focusRequest?: BrowseGridFocus) {
  return (
    <BrowseGrid
      label="Folder items"
      columns={["Name", "Kind"]}
      count={count}
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
