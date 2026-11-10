import { createRef } from "react";
import type { RefObject } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Action, BrowseTable } from "../src/components";

const columns = [
  "Name",
  "Type",
  "Logical size",
  "Allocated size",
  "Modified",
  "Availability",
];
const fixture = [
  "폴더".repeat(120),
  "directory",
  "At least 9223372036854775807 bytes",
  "9223372036854775807 bytes",
  "2026-11-10T12:34:56.789Z",
  "Not enumerated · Excluded · Availability unknown",
] as const;

function browseTable(
  count: number,
  query = "first",
  scrollFooter?: RefObject<HTMLButtonElement | null>,
) {
  return (
    <BrowseTable
      key={query}
      label="Folder items"
      columns={columns}
      rows={Array.from({ length: count }, (_, index) => index)}
      totalRows={count}
      {...(scrollFooter ? { scrollFooter } : {})}
      getRowKey={(index) => "result:" + index}
      renderCells={(index) => [
        <Action onPress={() => undefined}>
          {index === 0 ? fixture[0] : "Row " + index}
        </Action>,
        ...fixture.slice(1),
      ]}
    />
  );
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function fakeGeometry() {
  let callback: FrameRequestCallback | undefined;
  let onResize: ResizeObserverCallback | undefined;
  const values = {
    top: 0,
    width: 800,
    firstHeight: 64,
    scrollY: 0,
    footerBottom: window.innerHeight - 4,
    documentHeight: 0,
  };
  vi.spyOn(window, "scrollY", "get").mockImplementation(() => values.scrollY);
  const documentRoot = document.scrollingElement ?? document.documentElement;
  vi.spyOn(documentRoot, "scrollHeight", "get").mockImplementation(
    () => values.documentHeight,
  );
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((next) => {
    callback = next;
    return 1;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {
    callback = undefined;
  });
  const scrollBy = vi.spyOn(window, "scrollBy").mockImplementation((_x, y) => {
    if (typeof y === "number") {
      values.top -= y;
      values.scrollY += y;
      values.footerBottom -= y;
    }
  });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(notify: ResizeObserverCallback) {
        onResize = notify;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this.dataset.footer !== undefined) {
        return DOMRect.fromRect({ y: values.footerBottom - 42, height: 42 });
      }
      if (this.tagName === "TBODY") return DOMRect.fromRect({ y: values.top });
      if (this.hasAttribute("data-row")) {
        return DOMRect.fromRect({
          height: this.dataset.row === "0" ? values.firstHeight : 64,
        });
      }
      return DOMRect.fromRect({ width: values.width });
    },
  );
  const flush = () =>
    act(() => {
      const frame = callback;
      callback = undefined;
      frame?.(0);
    });
  const resized = () => {
    onResize?.([], {} as ResizeObserver);
    flush();
  };
  return { values, flush, resized, scrollBy };
}

test("renders all six values and their narrow-layout labels without a spacer for two rows", () => {
  const { container } = render(browseTable(2));
  const table = screen.getByRole("table");
  expect(table.getAttribute("aria-rowcount")).toBe("3");
  expect(
    within(table)
      .getAllByRole("columnheader")
      .map((cell) => cell.textContent),
  ).toEqual(columns);
  const row = within(table).getAllByRole("row")[1];
  if (!row) throw new Error("Expected a browse row");
  const cells = within(row).getAllByRole("cell");
  expect(
    cells.map((cell, index) => cell.textContent?.slice(columns[index]?.length)),
  ).toEqual(fixture);
  expect(cells.every((cell) => cell.hasAttribute("headers"))).toBe(true);
  expect(cells.every((cell) => !cell.hasAttribute("style"))).toBe(true);
  expect(container.querySelector(".du-browse-table__gap")).toBeNull();
  expect(container.querySelector(".du-table-viewport")).toBeNull();
});

test("keeps only a window and the focused row when scrolling, and releases it on blur", () => {
  const { values, flush, scrollBy } = fakeGeometry();
  render(browseTable(200));
  const table = screen.getByRole("table");
  expect(table.querySelectorAll("[data-row]")).toHaveLength(20);
  flush();
  const focused = screen.getByRole("button", { name: fixture[0] });
  act(() => focused.focus());
  values.top = -64 * 100;
  fireEvent.scroll(window);
  flush();

  expect(screen.getByRole("button", { name: "Row 100" })).toBeDefined();
  expect(screen.getByRole("button", { name: fixture[0] })).toBe(focused);
  expect(document.activeElement).toBe(focused);
  expect(table.querySelectorAll("[data-row]").length).toBeLessThan(40);
  expect(scrollBy).not.toHaveBeenCalled();

  act(() => screen.getByRole("button", { name: "Row 100" }).focus());
  expect(screen.queryByRole("button", { name: fixture[0] })).toBeNull();
});

test("corrects the document anchor when a measured earlier row grows", () => {
  const { values, flush, resized, scrollBy } = fakeGeometry();
  render(browseTable(200));
  flush();
  values.top = -64 * 4 - 8;
  fireEvent.scroll(window);
  flush();
  expect(scrollBy).not.toHaveBeenCalled();

  values.width = 440;
  values.firstHeight = 144;
  resized();

  expect(scrollBy).toHaveBeenCalledExactlyOnceWith(0, 80);
  expect(values.top).toBe(-64 * 4 - 8 - 80);
  expect(screen.getByRole("button", { name: "Row 4" })).toBeDefined();
});

test("page append retains measurements and a new query resets the render window", () => {
  const { values, flush, scrollBy } = fakeGeometry();
  values.firstHeight = 144;
  const { rerender } = render(browseTable(200));
  flush();
  values.top = -64 * 100;
  fireEvent.scroll(window);
  flush();

  const gap = () =>
    screen
      .getByRole("table")
      .querySelector<HTMLTableCellElement>(".du-browse-table__gap td")?.style
      .blockSize;
  const original = gap();
  expect(original).toBe("5712px");
  expect(screen.queryByRole("button", { name: fixture[0] })).toBeNull();

  rerender(browseTable(201));
  flush();
  expect(gap()).toBe(original);
  expect(scrollBy).not.toHaveBeenCalled();

  values.top = 0;
  rerender(browseTable(2, "second"));
  expect(gap()).toBeUndefined();
  expect(screen.getByRole("button", { name: fixture[0] })).toBeDefined();
  expect(screen.queryByRole("button", { name: "Row 100" })).toBeNull();
});

test.each(["keyboard", "wheel", "pointer"] as const)(
  "remeasurement reveals the footer only for current keyboard focus intent (%s)",
  (intent) => {
    const { values, flush, resized, scrollBy } = fakeGeometry();
    const footerRef = createRef<HTMLButtonElement>();
    render(
      <>
        {browseTable(200, "first", footerRef)}
        <button ref={footerRef} data-footer>
          Load more
        </button>
      </>,
    );
    flush();
    const footer = screen.getByRole("button", { name: "Load more" });
    vi.spyOn(footer, "matches").mockImplementation(
      (selector) => selector === ":focus-visible" && intent !== "pointer",
    );
    act(() => footer.focus());
    if (intent === "wheel") fireEvent.wheel(footer, { deltaY: -120 });

    values.firstHeight += 80;
    values.footerBottom += 80;
    resized();

    expect(document.activeElement).toBe(footer);
    if (intent === "keyboard") {
      expect(scrollBy).toHaveBeenCalledExactlyOnceWith(0, 80);
      expect(footer.getBoundingClientRect().bottom).toBe(
        window.innerHeight - 4,
      );
      fireEvent.scroll(window);
      flush();
      resized();
      expect(scrollBy).toHaveBeenCalledTimes(1);
    } else {
      expect(scrollBy).not.toHaveBeenCalled();
    }
  },
);

test("a document-end scroll stays at the end while rows remeasure, but does not follow appended rows", () => {
  const { values, flush, resized, scrollBy } = fakeGeometry();
  const footer = createRef<HTMLButtonElement>();
  const { rerender } = render(browseTable(200, "first", footer));
  flush();
  values.scrollY = 900;
  values.documentHeight = window.innerHeight + values.scrollY;
  fireEvent.scroll(window);

  values.firstHeight += 80;
  values.documentHeight += 80;
  resized();
  expect(scrollBy).toHaveBeenCalledExactlyOnceWith(0, 80);
  expect(values.scrollY).toBe(980);

  fireEvent.scroll(window);
  flush();
  resized();
  expect(scrollBy).toHaveBeenCalledTimes(1);

  fireEvent.scroll(window);
  values.documentHeight += 200 * 64;
  rerender(browseTable(400, "first", footer));
  flush();
  expect(values.scrollY).toBe(980);
  expect(scrollBy).toHaveBeenCalledTimes(1);
});
