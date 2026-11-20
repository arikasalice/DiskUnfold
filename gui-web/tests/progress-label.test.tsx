import { readFileSync } from "node:fs";
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

test("F06 design README 31: indeterminate progress moves only when motion is allowed", () => {
  const css = readFileSync("src/theme/workspace.css", "utf8");
  const moving = render(<Progress label="Checking items" showLabel={false} />);
  const indicator = moving.container.querySelector(".du-progress__indicator");

  expect(indicator?.getAttribute("style")).toBeNull();
  expect(css).toContain(
    "animation: du-indeterminate 1.4s ease-in-out infinite;",
  );
  expect(css).toContain("transform: translateX(-110%);");
  expect(css).toMatch(
    /@media \(prefers-reduced-motion: reduce\) \{\s*\.du-progress__indicator \{\s*animation: none;/,
  );
  expect(css).toMatch(
    /@media \(prefers-reduced-motion: reduce\) \{\s*\.du-progress:not\(\.du-progress--determinate\) \.du-progress__indicator \{\s*display: none;/,
  );
  expect(css).toMatch(
    /:root\[data-du-motion="reduce"\][\s\S]*?\.du-progress__indicator \{\s*display: none;/,
  );
});
