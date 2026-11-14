import { afterEach, expect, test } from "vitest";
import {
  BROWSE_SCREEN,
  browseKeyTable,
  classifyKeyStage,
  createKeyTable,
  dispatchKey,
  findDuplicateBindings,
} from "../src/components";
import { DuplicateKeyBindingError } from "../src/components/key-table";
import type { KeyBinding } from "../src/components";
import { registeredKeyTables } from "../src/components/key-table";

afterEach(() => {
  document.body.replaceChildren();
});

const row = (
  key: string,
  action: string,
  modifiers: KeyBinding["modifiers"] = [],
  screen = "fixture",
): KeyBinding => ({
  screen,
  key,
  modifiers,
  action,
  descriptionKey: `ui.fixture.keys.${action}`,
});

const fixtureTable = createKeyTable([
  row("q", "quit", ["ctrl"]),
  row("?", "help"),
  row("Delete", "remove"),
  row("Delete", "remove_all", ["shift"]),
]);

/** Dispatches a real keydown and returns what the dispatcher decided. */
function press(
  target: Element,
  init: KeyboardEventInit & { keyCode?: number },
  screen = "fixture",
  table = fixtureTable,
) {
  let outcome: ReturnType<typeof dispatchKey> | undefined;
  target.addEventListener(
    "keydown",
    (event) => {
      outcome = dispatchKey(event as KeyboardEvent, table, screen);
    },
    { once: true },
  );
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    ...init,
  });
  if (init.keyCode !== undefined)
    Object.defineProperty(event, "keyCode", { value: init.keyCode });
  target.dispatchEvent(event);
  return outcome;
}

function page(html: string) {
  document.body.innerHTML = html;
  return document.body;
}

test("composition owns the key before the input, dialog and screen table", () => {
  const body = page(
    '<div role="dialog"><input id="entry"><button id="plain">x</button></div>',
  );
  const entry = body.querySelector("#entry")!;
  expect(press(entry, { key: "Delete", isComposing: true })).toEqual({
    stage: "composition",
  });
  expect(press(entry, { key: "Delete", keyCode: 229 })).toEqual({
    stage: "composition",
  });
  expect(press(entry, { key: "Process" })).toEqual({ stage: "composition" });
});

test("a text entry owns its keys before the dialog and the screen table", () => {
  const body = page('<div role="dialog"><input id="entry"></div>');
  expect(press(body.querySelector("#entry")!, { key: "Delete" })).toEqual({
    stage: "input",
  });
});

test("an open dialog owns keys before the screen table", () => {
  const body = page(
    '<div role="dialog"><button id="inside">x</button></div><button id="outside">y</button>',
  );
  expect(press(body.querySelector("#inside")!, { key: "Delete" })).toEqual({
    stage: "dialog",
  });
  // A modal dialog also blocks keys aimed at the page behind it.
  expect(press(body.querySelector("#outside")!, { key: "Delete" })).toEqual({
    stage: "dialog",
  });
});

test("the screen table runs after the other stages and unmatched keys are ignored", () => {
  const body = page('<button id="plain">x</button>');
  const target = body.querySelector("#plain")!;
  expect(press(target, { key: "Delete" })).toEqual({
    stage: "table",
    action: "remove",
  });
  expect(press(target, { key: "Delete", shiftKey: true })).toEqual({
    stage: "table",
    action: "remove_all",
  });
  expect(press(target, { key: "Delete", altKey: true })).toEqual({
    stage: "ignored",
  });
  expect(press(target, { key: "F9" })).toEqual({ stage: "ignored" });
  expect(press(target, { key: "Delete" }, "another-screen")).toEqual({
    stage: "ignored",
  });
});

test("a key another handler already cancelled is ignored", () => {
  const body = page('<button id="plain">x</button>');
  const target = body.querySelector("#plain")!;
  target.addEventListener("keydown", (event) => event.preventDefault(), {
    capture: true,
  });
  expect(press(target, { key: "Delete" })).toEqual({ stage: "ignored" });
});

test("letter keys match the logical key, not the physical position", () => {
  const body = page('<button id="plain">x</button>');
  const target = body.querySelector("#plain")!;
  // On an AZERTY layout the key at the physical Q position produces "a".
  expect(press(target, { key: "a", code: "KeyQ", ctrlKey: true })).toEqual({
    stage: "ignored",
  });
  expect(press(target, { key: "q", code: "KeyA", ctrlKey: true })).toEqual({
    stage: "table",
    action: "quit",
  });
  expect(press(target, { key: "Q", ctrlKey: true, shiftKey: false })).toEqual({
    stage: "table",
    action: "quit",
  });
  // Shift is part of the value of a printable non-letter key.
  expect(press(target, { key: "?", shiftKey: true })).toEqual({
    stage: "table",
    action: "help",
  });
});

test("two rows with the same screen, key and modifiers fail the table", () => {
  const duplicate = [row("Enter", "open"), row("Enter", "run")];
  expect(findDuplicateBindings(duplicate)).toHaveLength(1);
  expect(() => createKeyTable(duplicate)).toThrow(DuplicateKeyBindingError);
  // Modifier order and letter case do not make a binding distinct.
  expect(() =>
    createKeyTable([
      row("k", "a", ["ctrl", "shift"]),
      row("K", "b", ["shift", "ctrl"]),
    ]),
  ).toThrow(DuplicateKeyBindingError);
  // The same keys on another screen are allowed.
  expect(() =>
    createKeyTable([row("Enter", "open"), row("Enter", "run", [], "other")]),
  ).not.toThrow();
});

test("a row needs an action and a description key", () => {
  expect(() =>
    createKeyTable([{ ...row("a", "x"), descriptionKey: "" }]),
  ).toThrow(RangeError);
  expect(() => createKeyTable([{ ...row("a", "x"), action: "" }])).toThrow(
    RangeError,
  );
});

test("every registered screen table is free of duplicate assignments", () => {
  expect(browseKeyTable.bindings.length).toBeGreaterThan(0);
  const tables = registeredKeyTables();
  expect(tables).toContain(browseKeyTable);
  for (const table of tables)
    expect(findDuplicateBindings(table.bindings)).toEqual([]);
});

test("the browse table keeps the keys the grid handled before", () => {
  const keys = browseKeyTable.bindings
    .filter(({ modifiers }) => modifiers.length === 0)
    .map(({ key }) => key)
    .sort();
  expect(keys).toEqual(
    [
      "ArrowUp",
      "ArrowDown",
      "ArrowLeft",
      "ArrowRight",
      "Home",
      "End",
      "PageUp",
      "PageDown",
      "Enter",
      " ",
    ].sort(),
  );
  for (const { screen } of browseKeyTable.bindings)
    expect(screen).toBe(BROWSE_SCREEN);
});

test("stage classification does not need a table", () => {
  const body = page('<button id="plain">x</button>');
  const target = body.querySelector("#plain")!;
  let stage: string | undefined;
  target.addEventListener("keydown", (event) => {
    stage = classifyKeyStage(event as KeyboardEvent);
  });
  target.dispatchEvent(new KeyboardEvent("keydown", { key: "x" }));
  expect(stage).toBe("screen");
});
