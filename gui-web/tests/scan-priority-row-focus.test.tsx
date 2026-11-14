import { act, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { gridRow, rowAction, setup } from "./scan-priority-fixture";

/*
 * React Aria keeps the input modality in module state, so this pointer focus
 * check runs in its own file where earlier keyboard tests cannot change it.
 */

afterEach(cleanup);

test("F02 FR8.HD1: focus stays in the row when a pressed row action goes away", async () => {
  await setup();
  const action = rowAction("Videos")!;
  // A pointer press as the browser delivers it: down, focus, up, click.
  const press = { pointerType: "mouse", width: 10, height: 10, pressure: 0.5 };
  fireEvent.pointerDown(action, press);
  fireEvent.mouseDown(action, press);
  act(() => action.focus());
  fireEvent.pointerUp(action, press);
  fireEvent.mouseUp(action, press);
  fireEvent.click(action, press);
  expect(rowAction("Videos")).toBeNull();
  // React Aria moves focus from the pressed action to its cell.
  expect(document.activeElement?.getAttribute("role")).toBe("gridcell");
  expect(gridRow("Videos").contains(document.activeElement)).toBe(true);
});
