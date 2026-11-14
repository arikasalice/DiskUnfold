import "./browse-scroll-fixture";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { flush } from "./groups-fixtures";
import type { Body } from "./groups-fixtures";
import { classified, mount, tab } from "./groups-view-helpers";

function memberBodies(fake: ReturnType<typeof mount>["fake"]) {
  return fake.calls
    .filter((call) => call.name === "group.members")
    .map((call) => call.body as Body);
}

test("F36 #29 review (1): the categories are tabs; arrow keys move between them and the panel follows the selected tab", async () => {
  const { fake } = await classified();
  expect(screen.getByRole("tablist", { name: "Categories" })).toBeDefined();
  expect(screen.queryByRole("radio")).toBeNull();
  const groups = tab("Groups");
  const overlaps = tab("Overlaps");
  expect(groups.getAttribute("aria-selected")).toBe("true");
  const panel = screen.getByRole("tabpanel");
  expect(panel.getAttribute("aria-labelledby")).toBe(groups.id);
  expect(
    within(panel).getByRole("table", { name: "Virtual group summary" }),
  ).toBeDefined();

  groups.focus();
  fireEvent.keyDown(groups, { key: "ArrowRight" });
  await waitFor(() =>
    expect(tab("Overlaps").getAttribute("aria-selected")).toBe("true"),
  );
  expect(document.activeElement).toBe(overlaps);
  const moved = screen.getByRole("tabpanel");
  expect(moved.getAttribute("aria-labelledby")).toBe(overlaps.id);
  expect(
    within(moved).getByRole("heading", { name: "Members: Overlaps" }),
  ).toBeDefined();
  // Only the groups tab shows the summary table.
  expect(
    screen.queryByRole("table", { name: "Virtual group summary" }),
  ).toBeNull();
  await waitFor(() =>
    expect(memberBodies(fake).at(-1)?.category).toEqual({ kind: "overlap" }),
  );

  fireEvent.keyDown(overlaps, { key: "End" });
  await waitFor(() =>
    expect(tab("Cannot decide").getAttribute("aria-selected")).toBe("true"),
  );
  await waitFor(() =>
    expect(memberBodies(fake).at(-1)?.category).toEqual({
      kind: "undecidable",
    }),
  );
  fireEvent.keyDown(tab("Cannot decide"), { key: "Home" });
  await waitFor(() =>
    expect(tab("Groups").getAttribute("aria-selected")).toBe("true"),
  );
  await screen.findByRole("table", { name: "Virtual group summary" });
});

test("F36 #29 review (4): Enter and Escape that belong to a Korean composition in a condition value do nothing", async () => {
  const { fake } = mount();
  await screen.findByText("Movies + Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const editor = await screen.findByRole("region", {
    name: "Edit group rules",
  });
  const value = within(editor).getAllByRole("textbox", {
    name: "Enter a value",
  })[0]!;
  fireEvent.compositionStart(value);
  fireEvent.change(value, { target: { value: "\u3147" } });
  fireEvent.keyDown(value, { key: "Enter", isComposing: true, keyCode: 229 });
  fireEvent.keyDown(value, { key: "Escape", isComposing: true, keyCode: 229 });
  await flush();
  expect(fake.count("group.rules.save")).toBe(0);
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(
    screen.getByRole("region", { name: "Edit group rules" }),
  ).toBeDefined();
  fireEvent.change(value, { target: { value: "\uc601\ud654" } });
  fireEvent.compositionEnd(value);
  expect((value as HTMLInputElement).value).toBe("\uc601\ud654");
  // A finished composition leaves the edit as an ordinary change, still not saved.
  expect(fake.count("group.rules.save")).toBe(0);
  // The Escape that ends a composition belongs to the IME; a later one is the user's.
  fireEvent.keyDown(value, { key: "a", keyCode: 65, isComposing: false });
  fireEvent.keyDown(value, { key: "Escape", keyCode: 27, isComposing: false });
  await screen.findByRole("alertdialog");
});
