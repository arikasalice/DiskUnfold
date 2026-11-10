import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { DataTable } from "../src/components";

afterEach(cleanup);

function table(size: number, renderCells = (row: string[]) => row) {
  return (
    <DataTable
      label="History"
      columns={["Job", "Scope"]}
      rows={Array.from({ length: size }, (_, i) => [`Job ${i}`, `Scope ${i}`])}
      renderCells={renderCells}
    />
  );
}

test("large tables bound DOM rows and preserve accessible logical positions", () => {
  const renderCells = vi.fn((row: string[]) => row);
  const { rerender } = render(table(10_000, renderCells));
  const viewport = screen.getByRole("region", { name: "History scroll area" });
  const element = screen.getByRole("table", { name: "History" });
  expect(element.getAttribute("aria-rowcount")).toBe("10001");
  expect(
    within(element)
      .getAllByRole("columnheader")
      .map((header) => header.textContent),
  ).toEqual(["Job", "Scope"]);
  expect(
    within(element)
      .getByText("Job 0")
      .closest("tr")
      ?.getAttribute("aria-rowindex"),
  ).toBe("2");
  expect(within(element).getAllByRole("row").length).toBeLessThanOrEqual(12);
  expect(renderCells.mock.calls.length).toBeLessThanOrEqual(11);
  expect(
    (within(element).getByText("Job 0") as HTMLElement).style.blockSize,
  ).toBe("144px");
  Object.defineProperty(viewport, "scrollTop", {
    value: 144 * 500,
    writable: true,
    configurable: true,
  });
  renderCells.mockClear();
  fireEvent.scroll(viewport);
  expect(
    within(element)
      .getByText("Job 500")
      .closest("tr")
      ?.getAttribute("aria-rowindex"),
  ).toBe("502");
  expect(within(element).queryByText("Job 0")).toBeNull();
  expect(within(element).getAllByRole("row").length).toBeLessThanOrEqual(12);
  expect(renderCells.mock.calls.length).toBeLessThanOrEqual(11);
  rerender(table(2));
  expect(within(element).getByText("Job 0")).toBeDefined();
  expect(within(element).getByText("Job 1")).toBeDefined();
});

test("empty and small tables do not create virtual spacer rows", () => {
  const { rerender, container } = render(table(0));
  expect(screen.getByRole("table").getAttribute("aria-rowcount")).toBe("1");
  expect(container.querySelectorAll(".du-table-spacer")).toHaveLength(0);
  rerender(table(2));
  expect(screen.getByRole("table").getAttribute("aria-rowcount")).toBe("3");
  expect(container.querySelectorAll(".du-table-spacer")).toHaveLength(0);
  expect(screen.getByText("Job 1")).toBeDefined();
});

test("typical maximum counts and session label remain in non-focusable cells", () => {
  const value = "9223372036854775807";
  const counts = `Completed ${value} · Overwritten ${value} · Failed ${value} · Skipped ${value} · Unprocessed ${value}`;
  const session =
    "Restore information available in this session · Follow-up: effect_check · Incomplete";
  render(
    <DataTable
      label="History"
      columns={["Counts", "Details"]}
      columnWidths={[420, 400]}
      rows={[[counts, session]]}
      renderCells={(row) => row}
    />,
  );
  const cell = screen.getByText(counts);
  expect(cell.textContent).toBe(counts);
  expect(cell.getAttribute("tabindex")).toBeNull();
  expect(screen.getByRole("region").getAttribute("tabindex")).toBe("0");
  expect(screen.getByText(session).textContent).toBe(session);
  const cols = screen.getByRole("table").querySelectorAll("col");
  expect(cols[0]?.style.width).toBe("420px");
  expect(cols[1]?.style.width).toBe("400px");
});
