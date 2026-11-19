import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { Progress } from "../src/components";

afterEach(cleanup);

test("F06 design README 31: an indeterminate bar shows what it waits for above a track, once and hidden from assistive technology", () => {
  const { container } = render(<Progress label="Loading chart" />);
  const bar = screen.getByRole("progressbar", { name: "Loading chart" });
  const shown = container.querySelector(".du-progress__label");
  expect(shown?.textContent).toBe("Loading chart…");
  expect(shown?.getAttribute("aria-hidden")).toBe("true");
  expect(bar.contains(shown)).toBe(true);
  cleanup();
  // A label that already ends with an ellipsis gets no second one.
  const again = render(<Progress label="Loading the rules…" />);
  expect(
    again.container.querySelector(".du-progress__label")?.textContent,
  ).toBe("Loading the rules…");
});

test("F06 design README 31: a screen that already shows the words, and a determinate bar, show no label", () => {
  const hidden = render(<Progress label="Classifying…" showLabel={false} />);
  expect(hidden.container.querySelector(".du-progress__label")).toBeNull();
  expect(
    screen.getByRole("progressbar", { name: "Classifying…" }),
  ).toBeTruthy();
  cleanup();
  const determinate = render(
    <Progress label="Scanning" value={3} maxValue={10} />,
  );
  expect(determinate.container.querySelector(".du-progress__label")).toBeNull();
});
