import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  Toast,
  TOAST_DURATION,
  TOAST_ERROR_DURATION,
} from "../src/components/toast";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test("F06 README 35: a short notice shows on each new version, stays five seconds and is hidden from assistive technology", () => {
  vi.useFakeTimers();
  const { container, rerender } = render(<Toast message="Hello" version={0} />);
  expect(container.querySelector(".du-toast")).toBeNull();
  rerender(<Toast message="Hello" version={1} />);
  const toast = container.querySelector(".du-toast");
  expect(toast?.textContent).toBe("Hello");
  expect(toast?.getAttribute("aria-hidden")).toBe("true");
  // The same notice again restarts its time.
  act(() => vi.advanceTimersByTime(TOAST_DURATION - 100));
  rerender(<Toast message="Hello" version={2} />);
  act(() => vi.advanceTimersByTime(TOAST_DURATION - 100));
  expect(container.querySelector(".du-toast")).not.toBeNull();
  act(() => vi.advanceTimersByTime(100));
  expect(
    container.querySelector(".du-toast")?.getAttribute("data-leaving"),
  ).toBe("true");
  act(() => vi.advanceTimersByTime(200));
  expect(container.querySelector(".du-toast")).toBeNull();
});

test("F06 README 35: the next key hides the notice", () => {
  vi.useFakeTimers();
  const { container, rerender } = render(<Toast message="Hello" version={0} />);
  rerender(<Toast message="Hello" version={1} />);
  fireEvent.keyDown(window, { key: "ArrowRight" });
  act(() => vi.advanceTimersByTime(200));
  expect(container.querySelector(".du-toast")).toBeNull();
});

// F02 README, PR #194 review: a failure may arrive while the user moves
// through a list, so keys do not hide it and it stays eight seconds.
test("a failure notice stays when a key is pressed and leaves after eight seconds", () => {
  vi.useFakeTimers();
  const failure = (version: number) => (
    <Toast message="Failed" version={version} icon="error" />
  );
  const { container, rerender } = render(failure(0));
  rerender(failure(1));
  fireEvent.keyDown(window, { key: "ArrowDown" });
  act(() => vi.advanceTimersByTime(TOAST_DURATION + 200));
  expect(container.querySelector(".du-toast")?.textContent).toBe("Failed");
  expect(
    container.querySelector(".du-toast")?.hasAttribute("data-leaving"),
  ).toBe(false);
  act(() => vi.advanceTimersByTime(TOAST_ERROR_DURATION - TOAST_DURATION));
  act(() => vi.advanceTimersByTime(200));
  expect(container.querySelector(".du-toast")).toBeNull();
});

test("a newer notice takes the place of a failure and keys hide it again", () => {
  vi.useFakeTimers();
  const { container, rerender } = render(
    <Toast message="Failed" version={0} icon="error" />,
  );
  rerender(<Toast message="Failed" version={1} icon="error" />);
  rerender(<Toast message="Done" version={2} icon="success" />);
  expect(container.querySelector(".du-toast")?.textContent).toBe("Done");
  fireEvent.keyDown(window, { key: "ArrowDown" });
  act(() => vi.advanceTimersByTime(200));
  expect(container.querySelector(".du-toast")).toBeNull();
});
