import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { StackedBar } from "../src/components/charts/stacked-bar";
import type { StackedBarSlice } from "../src/components/charts/stacked-bar";

afterEach(cleanup);

const slice = (
  key: string,
  value: number,
  look: Partial<StackedBarSlice> = {},
): StackedBarSlice => ({
  key,
  tone: "1",
  pattern: null,
  patternAlways: false,
  value,
  ...look,
});

function parts(container: HTMLElement) {
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      ".du-stacked-bar__slice, .du-stacked-bar__rest",
    ),
  ).map((part) => [part.dataset["key"] ?? "rest", Number(part.style.flexGrow)]);
}

test("README 13: slices keep their order and length, and an empty slice is left out", () => {
  const { container } = render(
    <StackedBar
      slices={[slice("video", 300), slice("none", 0), slice("document", 100)]}
    />,
  );
  expect(parts(container)).toEqual([
    ["video", 0.75],
    ["document", 0.25],
  ]);
  const bar = container.querySelector(".du-stacked-bar");
  // Decorative: the screen says the same in text.
  expect(bar?.getAttribute("aria-hidden")).toBe("true");
  expect(bar?.getAttribute("data-height")).toBe("8");
});

test("README 54: the part of the whole beyond the slices is drawn last, never as a slice", () => {
  const { container } = render(
    <StackedBar slices={[slice("document", 30)]} total={120} height={12} />,
  );
  expect(parts(container)).toEqual([
    ["document", 0.25],
    ["rest", 0.75],
  ]);
  // A whole no larger than its slices leaves no rest.
  cleanup();
  const exact = render(<StackedBar slices={[slice("a", 5)]} total={5} />);
  expect(parts(exact.container)).toEqual([["a", 1]]);
});

test("README 44: a slice draws its pattern when patterns are on, or always for a neutral class", () => {
  const slices = [
    slice("image", 1, { tone: "3", pattern: "dots" }),
    slice("unavailable", 1, {
      tone: "neutral",
      pattern: "dense-hatch",
      patternAlways: true,
    }),
  ];
  const patterned = (container: HTMLElement) =>
    Array.from(container.querySelectorAll(".du-stacked-bar__slice"))
      .filter((part) => part.querySelector(".du-stacked-bar__pattern"))
      .map((part) => part.getAttribute("data-key"));
  const off = render(<StackedBar slices={slices} />);
  expect(patterned(off.container)).toEqual(["unavailable"]);
  cleanup();
  const on = render(<StackedBar slices={slices} patterns />);
  expect(patterned(on.container)).toEqual(["image", "unavailable"]);
  const fill = on.container
    .querySelector(".du-stacked-bar__pattern")
    ?.getAttribute("fill");
  const id = /^url\(#(.+)\)$/u.exec(fill ?? "")?.[1];
  expect(id && on.container.querySelector(`[id="${id}"]`)).toBeTruthy();
});

test("nothing to draw draws nothing", () => {
  const { container } = render(<StackedBar slices={[slice("a", 0)]} />);
  expect(container.innerHTML).toBe("");
});

test("a bar in a table cell takes the length it is given", () => {
  const { container } = render(
    <StackedBar slices={[slice("a", 1)]} width={96} />,
  );
  const bar = container.querySelector<HTMLElement>(".du-stacked-bar");
  expect(bar?.style.width).toBe("96px");
});
