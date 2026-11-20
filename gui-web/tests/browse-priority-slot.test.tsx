import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { PrioritySlot } from "../src/features/browse/PrioritySlot";
import type { FolderQueryRow } from "../src/ipc";
import type { ScanPriority } from "../src/state/scan-priority";
import {
  gridRow,
  ref,
  requestFirst,
  rowAction,
  setup,
} from "./scan-priority-fixture";

/*
 * F02 design README 30: with the availability column gone, the "Analyze
 * first" badge and row action share one place at the end of the name cell,
 * before the row menu, while the list shows a running or paused scan.
 */

afterEach(cleanup);

const nameCell = (name: string) => within(gridRow(name)).getByRole("rowheader");

test("F02 FR8.HD1: every row of a running scan keeps the place in its name cell, before the row menu", async () => {
  await setup();
  for (const name of ["Photos", "memo.txt", "Documents"]) {
    const slot = nameCell(name).querySelector(".du-row-slot");
    expect(slot).not.toBeNull();
    const menu = within(nameCell(name)).getByRole("button", {
      name: `Actions for ${name}`,
    });
    expect(
      slot!.compareDocumentPosition(menu) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  }
  // The row action is in the name cell, and a file or a collected folder
  // keeps an empty place.
  expect(nameCell("Photos").contains(rowAction("Photos"))).toBe(true);
  expect(
    nameCell("memo.txt")
      .querySelector(".du-row-slot")!
      .getAttribute("data-holds"),
  ).toBeNull();
  expect(
    nameCell("Documents")
      .querySelector(".du-row-slot")!
      .getAttribute("data-holds"),
  ).toBeNull();
});

test("F02 FR8.HD1: the reserved widths are hidden labels that are neither text nor read", async () => {
  await setup();
  const reserve = nameCell("Photos").querySelector(".du-row-slot__reserve")!;
  expect(reserve.getAttribute("aria-hidden")).toBe("true");
  expect(reserve.textContent).toBe("");
  expect(
    [...reserve.children].map((label) => label.getAttribute("data-label")),
  ).toEqual(["Analyze first requested", "Analyzing first", "Analyze first"]);
  expect(screen.queryByText("Analyze first requested")).toBeNull();
});

test("F02 FR8.HD1: a requested folder shows an info badge in its name, read with the row, and no row action", async () => {
  const { answers } = await setup();
  await requestFirst("Photos");
  await act(async () => answers[0]?.resolve({ controlResult: "accepted" }));
  const badge = within(nameCell("Photos")).getByText("Analyze first requested");
  expect(badge.classList.contains("du-badge--info")).toBe(true);
  expect(
    within(gridRow("Photos")).getAllByText("Analyze first requested"),
  ).toHaveLength(1);
  // The row is named by its name and badge only. The row action and the
  // menu button stay in the tree under their own names.
  expect(
    screen.getByRole("row", { name: "Photos Analyze first requested" }),
  ).toBe(gridRow("Photos"));
  expect(screen.getByRole("row", { name: "Videos" })).toBe(gridRow("Videos"));
  expect(rowAction("Videos")).not.toBeNull();
  expect(screen.getByRole("row", { name: "memo.txt" })).toBe(
    gridRow("memo.txt"),
  );
  expect(
    within(gridRow("Photos")).getByRole("button", {
      name: "Actions for Photos",
    }),
  ).toBeDefined();
  // A receipt the engine did not confirm keeps the badge in the place; the
  // row menu still offers the request again.
  expect(rowAction("Photos")).toBeNull();
  fireEvent.contextMenu(gridRow("Photos"));
  const menu = await screen.findByRole("menu", { name: "Actions for Photos" });
  expect(
    within(menu).getByRole("menuitem", { name: "Analyze first" }),
  ).toBeDefined();
});

test("F02 FR8.HD1: a finished result keeps no place, so the name gets its width", async () => {
  const { rerender } = await setup();
  rerender(undefined);
  expect(nameCell("Photos").querySelector(".du-row-slot")).toBeNull();
  expect(document.querySelector(".du-row-slot")).toBeNull();
});

test("F02 FR8.HD1: a folder the engine reports as its current area reads Analyzing first, without a row action", () => {
  const row: FolderQueryRow = {
    item: ref(1),
    displayName: { state: "present", value: "Photos" },
    kind: "directory",
    logicalSize: { state: "unknown", reasonKey: "ref.unavailable" },
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: { state: "unknown", reasonKey: "ref.unavailable" },
    flags: { unenumerated: { state: "absent" }, excluded: { state: "absent" } },
    correspondence: { state: "unknown", reasonKey: "ref.unavailable" },
  };
  const priority: ScanPriority = {
    canRequest: () => true,
    superseded: () => false,
    showLatest: undefined,
    requested: () => true,
    status: () => "analyzing",
    requestedArea: undefined,
    request: () => undefined,
    notice: undefined,
  };
  render(
    <PrioritySlot
      row={row}
      priority={priority}
      readOnly={false}
      badgeId="badge"
    />,
  );
  const badge = screen.getByText("Analyzing first");
  expect(badge.classList).toContain("du-badge--info");
  // The row names itself with this badge the same way as with "requested".
  expect(badge.id).toBe("badge");
  expect(screen.queryByRole("button")).toBeNull();
});
