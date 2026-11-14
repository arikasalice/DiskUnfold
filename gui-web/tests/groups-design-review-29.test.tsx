import "./browse-scroll-fixture";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { mount } from "./groups-view-helpers";

test("F36 #29 design review (2): opening the screen focuses its title and the screen is not a focus stop", async () => {
  mount();
  await screen.findByText("Movies + Archive");
  const title = screen.getByRole("heading", { name: "Group by my rules" });
  expect(document.activeElement).toBe(title);
  expect(title.getAttribute("tabindex")).toBe("-1");
  const root = document.querySelector(".du-groups")!;
  expect(root.hasAttribute("tabindex")).toBe(false);
  expect(root.contains(document.activeElement)).toBe(true);
});

test("F36 #29 design review (2): the editor takes the focus when it opens and the title when it closes", async () => {
  mount();
  await screen.findByText("Movies + Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const editor = await screen.findByRole("region", {
    name: "Edit group rules",
  });
  await waitFor(() => expect(document.activeElement).toBe(editor));
  fireEvent.keyDown(editor, { key: "Escape" });
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Edit group rules" }),
    ).toBeNull(),
  );
  expect(document.activeElement).toBe(
    screen.getByRole("heading", { name: "Group by my rules" }),
  );
});

test("F36 #29 design review (3): the group picker and the add, move and remove actions come before the group name", async () => {
  mount();
  await screen.findByText("Movies + Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const editor = await screen.findByRole("region", {
    name: "Edit group rules",
  });
  const name = within(editor).getByRole("textbox", { name: "Group name" });
  const before = [
    within(editor).getByRole("combobox", { name: "Group being edited" }),
    within(editor).getByRole("button", { name: "Add group" }),
    within(editor).getByRole("button", { name: /^Move .* up$/ }),
    within(editor).getByRole("button", { name: /^Move .* down$/ }),
    within(editor).getByRole("button", { name: /^Remove (?!condition)/ }),
  ];
  let last: Element = before[0]!;
  for (const element of before.slice(1)) {
    expect(
      last.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    last = element;
  }
  expect(
    last.compareDocumentPosition(name) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  // The keyboard order is the document order: nothing sets a positive tab index.
  expect(
    editor.querySelector("[tabindex]:not([tabindex='-1']):not([tabindex='0'])"),
  ).toBeNull();
});
