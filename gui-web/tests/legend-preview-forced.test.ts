// @vitest-environment node
import { expect, test } from "vitest";
import { allRules, targets } from "./style-rules";

/*
 * Forced colours ignore opacity, so a preview or choice that fades the other
 * classes gives them a GrayText edge instead (F06 README 44.4, #383). The rules
 * are checked as rules: the forced-colours rendering itself is in the captures.
 */

const all = allRules();

test("README 44.4, #383: in forced colours a faded slice or shape takes a GrayText edge", () => {
  const slice = targets(all, /\[data-dimmed\] > \.du-slice-stack__slice$/u);
  expect(
    slice.some((rule) => rule.declarations.get("stroke") === "GrayText"),
  ).toBe(true);
  const shape = targets(
    all,
    /\.du-geometry-shape\[data-dimmed\] > rect:first-child$/u,
  );
  expect(
    shape.some((rule) => rule.declarations.get("stroke") === "GrayText"),
  ).toBe(true);
  // And the fade itself is lifted there, so the edge is what tells them apart.
  const lifted = targets(all, /^\.du-slice-stack \[data-dimmed\]$/u);
  expect(lifted.some((rule) => rule.declarations.get("opacity") === "1")).toBe(
    true,
  );
  // The shown class itself takes a 2 px Highlight edge, slices and shapes alike.
  for (const pattern of [
    /\[data-chosen\] > \.du-slice-stack__slice$/u,
    /\.du-geometry-shape\[data-highlighted\] > rect:first-child$/u,
  ]) {
    const rule = targets(all, pattern).find(
      (candidate) => candidate.declarations.get("stroke") === "Highlight",
    );
    expect(rule?.declarations.get("stroke-width"), String(pattern)).toBe("2");
  }
});
