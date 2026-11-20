import axe from "axe-core";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ConfirmDialog, DateField, PathField } from "../src/components";

afterEach(cleanup);

function renderDialog(onConfirm = vi.fn(), onCancel = vi.fn()) {
  render(
    <ConfirmDialog
      title="Approve cleanup"
      confirmLabel="Approve"
      cancelLabel="Cancel"
      onConfirm={onConfirm}
      onCancel={onCancel}
    >
      <PathField
        label="Note"
        description="Optional note."
        value=""
        onChange={() => {}}
        isDisabled={false}
      />
      <DateField label="Date" value="" onChange={() => {}} />
    </ConfirmDialog>,
  );
  return { onConfirm, onCancel };
}

function getFirstDateSegment() {
  const [segment] = screen.getAllByRole("spinbutton");
  if (!segment) throw new Error("The date field should expose a segment.");
  return segment;
}

function getDialogEntries() {
  return [screen.getByLabelText("Note"), getFirstDateSegment()] as const;
}

test("the safe choice takes the initial focus", async () => {
  renderDialog();
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Cancel" }),
    ),
  );
});

test("an absent body is omitted and existing button variants remain the defaults", () => {
  render(
    <ConfirmDialog
      title="Approve cleanup"
      confirmLabel="Approve"
      cancelLabel="Cancel"
      onConfirm={() => {}}
      onCancel={() => {}}
    >
      {null}
    </ConfirmDialog>,
  );

  expect(
    screen
      .getByRole("alertdialog")
      .querySelector(".du-inspection-dialog__body"),
  ).toBeNull();
  expect(
    screen
      .getByRole("button", { name: "Cancel" })
      .classList.contains("du-action--secondary"),
  ).toBe(true);
  expect(
    screen
      .getByRole("button", { name: "Approve" })
      .classList.contains("du-action--primary"),
  ).toBe(true);
});

test("Enter in any entry inside the dialog does not confirm or submit", () => {
  const { onConfirm } = renderDialog();
  const [note, dateSegment] = getDialogEntries();
  expect(fireEvent.keyDown(note, { key: "Enter", keyCode: 13 })).toBe(false);
  fireEvent.keyDown(dateSegment, { key: "Enter", keyCode: 13 });
  expect(onConfirm).not.toHaveBeenCalled();
});

test("the confirm button requests the action by Enter, Space and a click", () => {
  const { onConfirm } = renderDialog();
  const confirm = screen.getByRole("button", { name: "Approve" });
  fireEvent.keyDown(confirm, { key: "Enter" });
  fireEvent.keyUp(confirm, { key: "Enter" });
  fireEvent.keyDown(confirm, { key: " " });
  fireEvent.keyUp(confirm, { key: " " });
  fireEvent.click(confirm);
  expect(onConfirm).toHaveBeenCalledTimes(3);
});

test("the dialog has no accessibility violations", async () => {
  renderDialog();
  const dialog = screen.getByRole("alertdialog");
  const results = await axe.run({
    include: [dialog],
    // React Aria adds a hidden text input beside the date segments; axe in
    // JSDOM treats this non-interactive implementation detail as visible.
    exclude: [[".du-date-picker__input > input[hidden][type='text']"]],
  });
  expect(results.violations).toEqual([]);
});

function keyDown(
  target: Element,
  key: string,
  init: { isComposing: boolean | undefined; keyCode: number },
) {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, "isComposing", { value: init.isComposing });
  Object.defineProperty(event, "keyCode", { value: init.keyCode });
  act(() => {
    target.dispatchEvent(event);
  });
  return event;
}

test("an Enter the input method is processing is not cancelled in any entry", () => {
  const { onConfirm } = renderDialog();
  for (const entry of getDialogEntries())
    for (const init of [
      { isComposing: true, keyCode: 13 },
      { isComposing: false, keyCode: 229 },
    ])
      expect(keyDown(entry, "Enter", init).defaultPrevented).toBe(false);
  expect(onConfirm).not.toHaveBeenCalled();
});

test("an Escape the input method is processing does not close the dialog, even in an entry with no composition handling", () => {
  const { onCancel } = renderDialog();
  const entry = getFirstDateSegment();
  for (const init of [
    { isComposing: true, keyCode: 229 },
    { isComposing: false, keyCode: 229 },
  ])
    keyDown(entry, "Escape", init);
  expect(onCancel).not.toHaveBeenCalled();
  keyDown(entry, "Escape", { isComposing: false, keyCode: 27 });
  expect(onCancel).toHaveBeenCalledTimes(1);
});
