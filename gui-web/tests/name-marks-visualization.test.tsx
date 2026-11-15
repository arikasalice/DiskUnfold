import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { GroupMembersPresenter } from "../src/features/visualization/GroupMembersPresenter";
import { ModelRows } from "../src/features/visualization/VisualizationModelPresenter";
import type { VizRow } from "../src/ipc";

afterEach(cleanup);

function marks(element: ParentNode) {
  return Array.from(element.querySelectorAll(".du-name__mark"));
}

/** The text a copy or a screen reader gets: everything except the drawn marks. */
function realText(element: Element) {
  const copy = element.cloneNode(true) as Element;
  for (const mark of marks(copy)) mark.remove();
  return copy.textContent;
}

/** Item rows " 보고서", "보고서 " and "보고서", and a group row whose engine label has edge spaces. */
function rows(): VizRow[] {
  const source = modelFixture().page.response.rows;
  const [lead, trail, plain, group] = source.map((row) => ({ ...row }));
  lead!.displayName = " 보고서";
  trail!.displayName = "보고서 ";
  plain!.displayName = "보고서";
  group!.itemRef = null;
  group!.groupRef = "fixture-group";
  group!.displayName = " Other ";
  return [lead!, trail!, plain!, group!];
}

function nameCells(table: HTMLElement) {
  return within(table)
    .getAllByRole("row")
    .slice(1)
    .map((row) => row.querySelector("td")!);
}

function expectMarked(cells: HTMLElement[]) {
  const [lead, trail, plain, group] = cells;
  expect(marks(lead!)).toHaveLength(1);
  expect(realText(lead!)).toBe(" 보고서, The name starts with 1 space");
  expect(marks(trail!)).toHaveLength(1);
  expect(realText(trail!)).toBe("보고서 , The name ends with 1 space");
  expect(marks(plain!)).toHaveLength(0);
  expect(plain!.textContent).toBe("보고서");
  // A group label comes from the engine and is not a file or folder name.
  expect(marks(group!)).toHaveLength(0);
  expect(group!.textContent).toBe(" Other ");
  for (const mark of cells.flatMap(marks))
    expect(mark.getAttribute("aria-hidden")).toBe("true");
}

test("F02: graph rows mark the edge spaces of item names only", () => {
  const input = modelFixture();
  input.page.response.rows = rows();
  render(
    <ModelRows
      page={input.page}
      model={input.frame.model}
      text={fixtureText}
      onOpenGroup={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Items in this folder" }));
  const table = screen.getByRole("table", { name: "Loaded chart rows" });
  expectMarked(nameCells(table));
  // The members action of the group keeps the label it was given.
  expect(
    within(table).getByRole("button", { name: "View items in Other" }),
  ).toBeDefined();
});

test("F02: group members mark the edge spaces of item names only", () => {
  render(
    <GroupMembersPresenter
      returnLabel="Back to graph"
      rows={rows()}
      rangeLabel="Rows 1–4 of 4"
      groupTotalLabel="Group total: 12 KB"
      sizeBasisLabel="Logical size"
      status="ready"
      canRetry={false}
      hasPrevious={false}
      hasNext={false}
      text={fixtureText}
      onClose={vi.fn()}
      onPrevious={vi.fn()}
      onNext={vi.fn()}
      onRetry={vi.fn()}
    />,
  );
  expectMarked(
    nameCells(screen.getByRole("table", { name: "Combined items" })),
  );
});
