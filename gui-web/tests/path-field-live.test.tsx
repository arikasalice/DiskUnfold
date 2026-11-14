import axe from "axe-core";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { PathField } from "../src/components";

afterEach(cleanup);

function renderField(error?: string) {
  return render(
    <PathField
      label="Folder"
      description="Type a folder path."
      value=""
      onChange={() => {}}
      isDisabled={false}
      error={error}
    />,
  );
}

test("the description is a polite live region while there is no error", () => {
  renderField();
  expect(
    screen.getByText("Type a folder path.").getAttribute("aria-live"),
  ).toBe("polite");
  expect(screen.queryByRole("alert")).toBeNull();
});

test("an error is the only announcement: the description stops being live", async () => {
  const { container } = renderField("This folder cannot be read.");
  expect(
    screen.getByText("Type a folder path.").getAttribute("aria-live"),
  ).toBe("off");
  expect(screen.getByRole("alert").textContent).toBe(
    "This folder cannot be read.",
  );
  const input = screen.getByRole("textbox", { name: "Folder" });
  expect(input.getAttribute("aria-invalid")).toBe("true");
  expect(
    container.querySelectorAll('[aria-live="polite"], [role="alert"]').length,
  ).toBe(1);
  expect((await axe.run(container)).violations).toEqual([]);
});
