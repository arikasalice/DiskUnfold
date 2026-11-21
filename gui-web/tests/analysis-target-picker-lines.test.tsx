import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { TargetPickerPreview } from "../preview/TargetPickerPreview";

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

test("F02 #350: a selected network volume reads as checking while its review is due", () => {
  window.history.replaceState(null, "", "/?scene=checking");
  render(<TargetPickerPreview />);
  const radio = screen.getByRole("radio", { name: "/Volumes/home" });
  expect(radio.closest("label")?.textContent).toContain("확인 중");
});

test("F02 #350: shown system volumes stand on one line apart from the cards", () => {
  render(<TargetPickerPreview />);
  expect(document.querySelectorAll(".du-line-choice")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: /모두 보기/ }));
  const lines = document.querySelectorAll(".du-choices__more .du-line-choice");
  expect(lines).toHaveLength(9);
  // The full path stays the accessible name of a line cut on screen.
  expect(
    screen.getByRole("radio", {
      name: "/Library/Developer/CoreSimulator/Volumes/iOS_22C150",
    }),
  ).toBeTruthy();
  expect(
    document.querySelectorAll(".du-choice:not(.du-line-choice)"),
  ).toHaveLength(2);
});
