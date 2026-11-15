// @vitest-environment node
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { usableSize } from "../src/features/visualization/chart-area";

test.each([
  // A 15 px vertical scrollbar takes width, a 17 px horizontal one takes height.
  [{ width: 1169, height: 605 }, [1169, 1154, 605, 605], 1154, 605],
  [{ width: 1169, height: 605 }, [1169, 1154, 605, 588], 1154, 588],
  [{ width: 640, height: 400 }, [0, 0, 0, 0], 640, 400],
  [{ width: 640, height: 400 }, [640, 640, 400, 400], 640, 400],
])(
  "the usable size of %o with metrics %o is %s x %s",
  (
    rect,
    [offsetWidth, clientWidth, offsetHeight, clientHeight],
    width,
    height,
  ) => {
    expect(
      usableSize(rect, {
        offsetWidth: offsetWidth!,
        clientWidth: clientWidth!,
        offsetHeight: offsetHeight!,
        clientHeight: clientHeight!,
      }),
    ).toEqual({ width, height });
  },
);

const css = readFileSync(
  new URL("../src/app/browse-visualization.css", import.meta.url),
  "utf8",
);
const rule = (selector: string) =>
  css.split("}").find((block) => block.includes(selector + " {")) ?? "";

test("the scroll area of the chart is a containing block, so hidden status regions cannot make the page scroll", () => {
  expect(rule(".du-visualization-viewport")).toMatch(/position:\s*relative/);
  expect(rule(".du-visualization-viewport")).toMatch(/overflow:\s*auto/);
});

test("the view title is the size of the folder list title and sits like it below the header", () => {
  expect(rule(".du-visualization-title .du-heading--2")).toMatch(
    /font-size:\s*20px/,
  );
  expect(rule(".du-visualization-title")).toMatch(/padding-block:\s*10px/);
});
