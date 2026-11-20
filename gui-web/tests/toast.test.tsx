import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Toast, TOAST_DURATION } from "../src/components/toast";

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
