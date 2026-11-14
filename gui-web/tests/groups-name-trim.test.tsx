import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { mount } from "./groups-view-helpers";

async function openNameField() {
  const mounted = mount();
  await screen.findByText("Movies + Archive");
  fireEvent.click(screen.getByRole("button", { name: "Edit rules" }));
  const name = (await screen.findByRole("textbox", {
    name: "Group name",
  })) as HTMLInputElement;
  return { ...mounted, name };
}

test("F36 FR3: typing keeps every space; leaving the name field trims only the surrounding space", async () => {
  const { name } = await openNameField();
  fireEvent.change(name, { target: { value: "보고서 " } });
  expect(name.value).toBe("보고서 ");
  fireEvent.change(name, { target: { value: "\u3000보고서 초안 " } });
  expect(name.value).toBe("\u3000보고서 초안 ");
  fireEvent.focus(name);
  fireEvent.blur(name);
  expect(name.value).toBe("보고서 초안");
  const picker = screen.getByRole("combobox", { name: "Group being edited" });
  expect(
    within(picker).getByRole("option", { name: "보고서 초안" }),
  ).toBeDefined();
});

test("F36 FR3: the save request carries the trimmed name even before the field loses focus", async () => {
  const { fake, name } = await openNameField();
  fireEvent.change(name, { target: { value: "\u00a0Films\t" } });
  expect(name.value).toBe("\u00a0Films\t");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(fake.count("group.rules.save")).toBe(1));
  const saved = fake.calls.find((call) => call.name === "group.rules.save");
  expect(saved?.body.rules[0].displayName).toBe("Films");
});

test("F36 FR3: surrounding space alone is no change, and a name of only spaces is no name", async () => {
  await openNameField();
  const editor = screen.getByRole("region", { name: "Edit group rules" });
  const name = within(editor).getByRole("textbox", { name: "Group name" });
  fireEvent.change(name, { target: { value: "\u3000Movies " } });
  expect(
    within(editor).getByText("There are no changes to save."),
  ).toBeDefined();
  fireEvent.change(name, { target: { value: " \u3000\u00a0" } });
  expect(within(editor).getByText("Enter a group name.")).toBeDefined();
  expect(within(editor).getByText("A group has no name.")).toBeDefined();
  expect(
    within(editor)
      .getByRole("button", { name: "Save" })
      .hasAttribute("disabled"),
  ).toBe(true);
});
