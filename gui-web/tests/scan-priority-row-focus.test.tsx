import { act, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { gridRow, rowAction, setup } from "./scan-priority-fixture";

/*
 * React Aria keeps the input modality in module state, so these press checks
 * run in their own file where earlier tests cannot change it.
 *
 * A browser leaves focus on the document body when the focused action is
 * removed, which jsdom does not reproduce reliably. These tests therefore
 * check that the action itself moves focus to its row.
 */

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function focusedRows(focus: { mock: { contexts: unknown[] } }) {
  return focus.mock.contexts.filter(
    (target) => target instanceof HTMLElement && target.role === "row",
  );
}

test("F02 FR8.HD1: a row action pressed by pointer moves focus to its row", async () => {
  await setup();
  const row = gridRow("Videos");
  const action = rowAction("Videos")!;
  const focus = vi.spyOn(HTMLElement.prototype, "focus");
  // A pointer press as the browser delivers it: down, focus, up, click.
  const press = { pointerType: "mouse", width: 10, height: 10, pressure: 0.5 };
  fireEvent.pointerDown(action, press);
  fireEvent.mouseDown(action, press);
  act(() => action.focus());
  fireEvent.pointerUp(action, press);
  fireEvent.mouseUp(action, press);
  fireEvent.click(action, press);
  expect(rowAction("Videos")).toBeNull();
  expect(focusedRows(focus)).toEqual([row]);
  expect(row.contains(document.activeElement)).toBe(true);
});

test("F02 FR8.HD1: a row action pressed by keyboard moves focus to its row", async () => {
  await setup();
  const row = gridRow("Photos");
  const action = rowAction("Photos")!;
  act(() => action.focus());
  const focus = vi.spyOn(HTMLElement.prototype, "focus");
  fireEvent.keyDown(action, { key: "Enter" });
  fireEvent.keyUp(action, { key: "Enter" });
  expect(rowAction("Photos")).toBeNull();
  expect(focusedRows(focus)).toEqual([row]);
  expect(row.contains(document.activeElement)).toBe(true);
});
