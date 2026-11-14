import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { ruleset } from "./groups-fixtures";
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

/** Opens the editor on stored rules whose first name has surrounding white space. */
async function openStoredName(stored: string) {
  const base = ruleset();
  const mounted = mount({
    "group.rules.get": () => ({
      ...base,
      rules: base.rules.map((rule, index) =>
        index === 0 ? { ...rule, displayName: stored } : rule,
      ),
    }),
  });
  fireEvent.click(await screen.findByRole("button", { name: "Edit rules" }));
  const editor = await screen.findByRole("region", {
    name: "Edit group rules",
  });
  const name = within(editor).getByRole("textbox", {
    name: "Group name",
  }) as HTMLInputElement;
  return { ...mounted, editor, name };
}

// The engine has always trimmed names it stores, so such a name can only come
// from a row another writer left behind.
test("F36 FR3: a stored name with surrounding space is shown as stored and trimmed when the field loses focus", async () => {
  const { name } = await openStoredName(" Movies\u3000");
  expect(name.value).toBe(" Movies\u3000");
  fireEvent.focus(name);
  fireEvent.blur(name);
  expect(name.value).toBe("Movies");
});

test("F36 FR3: trimming a stored name is no change: nothing is unsaved and Save stays unavailable", async () => {
  const { editor, name } = await openStoredName("\tMovies ");
  fireEvent.focus(name);
  fireEvent.blur(name);
  expect(name.value).toBe("Movies");
  expect(within(editor).queryByText("Not saved")).toBeNull();
  expect(
    within(editor).getByText("There are no changes to save."),
  ).toBeDefined();
  expect(
    within(editor)
      .getByRole("button", { name: "Save" })
      .hasAttribute("disabled"),
  ).toBe(true);
});

// jsdom has no input method: these tests replay the event order a webview
// reports and check what the field does with it, not what an IME delivers.
test("F36 FR3: leaving during a composition keeps the composing text and trims the composed value once it ends", async () => {
  const { name } = await openStoredName(" Movies");
  fireEvent.focus(name);
  fireEvent.compositionStart(name);
  fireEvent.change(name, { target: { value: " Movies 보" } });
  fireEvent.blur(name);
  expect(name.value).toBe(" Movies 보");
  fireEvent.change(name, { target: { value: " Movies 봄" } });
  fireEvent.compositionEnd(name);
  expect(name.value).toBe("Movies 봄");
});

test("F36 FR3: a composition that ends before the field loses focus is trimmed on leaving", async () => {
  const { name } = await openStoredName(" Movies");
  fireEvent.focus(name);
  fireEvent.compositionStart(name);
  fireEvent.change(name, { target: { value: " Movies 보" } });
  fireEvent.change(name, { target: { value: " Movies 봄" } });
  fireEvent.compositionEnd(name);
  expect(name.value).toBe(" Movies 봄");
  fireEvent.blur(name);
  expect(name.value).toBe("Movies 봄");
});

test("F36 FR3: returning to the field before the composition ends leaves the name for the next time focus leaves", async () => {
  const { name } = await openStoredName(" Movies");
  fireEvent.focus(name);
  fireEvent.compositionStart(name);
  fireEvent.change(name, { target: { value: " Movies 보" } });
  fireEvent.blur(name);
  fireEvent.focus(name);
  fireEvent.change(name, { target: { value: " Movies 봄" } });
  fireEvent.compositionEnd(name);
  expect(name.value).toBe(" Movies 봄");
  fireEvent.blur(name);
  expect(name.value).toBe("Movies 봄");
});
