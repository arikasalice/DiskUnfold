import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { ExplainedText, Readout } from "../src/components";

/*
 * A label can carry an explanation that shows on pointer hover or keyboard
 * focus, stays while the pointer is on it, closes with Escape and is the
 * label's accessible description (WCAG 1.4.13).
 */

afterEach(cleanup);

const explanation = "These weren't analyzed.";

function label() {
  return screen.getByText("Exclusions", { selector: '[role="note"]' });
}

test("the text is a tab stop that reads as a note with the explanation as its description", () => {
  render(<ExplainedText explanation={explanation}>Exclusions</ExplainedText>);
  const text = label();
  expect(text.tabIndex).toBe(0);
  expect(screen.queryByRole("button")).toBeNull();
  const description = document.getElementById(
    text.getAttribute("aria-describedby")!,
  )!;
  expect(description.textContent).toBe(explanation);
  // The description is in the document before anything is shown.
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(description.className).toBe("du-visually-hidden");
});

test("keyboard focus shows the explanation, Escape hides it and focus shows it again", async () => {
  render(<ExplainedText explanation={explanation}>Exclusions</ExplainedText>);
  const text = label();
  act(() => text.focus());
  expect((await screen.findByRole("tooltip")).textContent).toBe(explanation);

  fireEvent.keyDown(text, { key: "Escape" });
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(document.activeElement).toBe(text);

  act(() => text.blur());
  act(() => text.focus());
  expect(await screen.findByRole("tooltip")).toBeDefined();
  act(() => text.blur());
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("the pointer shows the explanation and can move onto it without closing it", async () => {
  render(<ExplainedText explanation={explanation}>Exclusions</ExplainedText>);
  const text = label();
  fireEvent.pointerEnter(text);
  const shown = await screen.findByRole("tooltip");

  // The pointer leaves the text for the explanation: it stays.
  fireEvent.pointerLeave(text);
  fireEvent.pointerEnter(shown);
  await act(() => new Promise((resolve) => setTimeout(resolve, 450)));
  expect(screen.getByRole("tooltip")).toBe(shown);

  // It closes once the pointer has left both.
  fireEvent.pointerLeave(shown);
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
});

test("Escape hides an explanation the pointer opened until the pointer has left", async () => {
  render(<ExplainedText explanation={explanation}>Exclusions</ExplainedText>);
  const text = label();
  fireEvent.pointerEnter(text);
  await screen.findByRole("tooltip");
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(screen.queryByRole("tooltip")).toBeNull();

  fireEvent.pointerLeave(text);
  await act(() => new Promise((resolve) => setTimeout(resolve, 450)));
  fireEvent.pointerEnter(text);
  expect(await screen.findByRole("tooltip")).toBeDefined();
});

test("only a row with an explanation makes its label a tab stop", () => {
  render(
    <Readout
      label="Scope"
      rows={[
        { label: "Scanned paths", value: "C:\\fixture" },
        { label: "Exclusions", value: "*.tmp", explanation },
      ]}
    />,
  );
  const [paths, exclusions] = Array.from(
    screen.getByLabelText("Scope").querySelectorAll("dt"),
  );
  expect(paths!.textContent).toBe("Scanned paths");
  expect(paths!.querySelector("[tabindex]")).toBeNull();
  expect(paths!.querySelector("[role]")).toBeNull();
  expect(exclusions!.querySelector('[role="note"]')).toBe(label());
  expect(document.querySelectorAll("[tabindex]")).toHaveLength(1);
});
