import axe from "axe-core";
import {
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

test("the safe choice takes the initial focus", async () => {
  renderDialog();
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Cancel" }),
    ),
  );
});

test("Enter in any entry inside the dialog does not confirm or submit", () => {
  const { onConfirm } = renderDialog();
  for (const name of ["Note", "Date"]) {
    const entry = screen.getByLabelText(name);
    const allowed = fireEvent.keyDown(entry, { key: "Enter", keyCode: 13 });
    expect(allowed).toBe(false);
  }
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
  expect((await axe.run(screen.getByRole("alertdialog"))).violations).toEqual(
    [],
  );
});
