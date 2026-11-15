import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { BROWSE_CARD_WIDTH } from "../src/components/browse-grid-viewport";

/*
 * Design review of PR #63: the folder table turns into cards in a window
 * 700px wide and stays a table in one 800px wide. The DOM test environment
 * computes no layout, so this checks the rules that decide it; the rendered
 * result is verified in a real webview.
 */

const css = (path: string) => readFileSync(path, "utf8");

/** The list's width in a window this wide: 16px of window frame and the app's side margins. */
function listWidth(width: number) {
  const viewport = width - 16;
  const margin = Math.min(36, Math.max(14, viewport * 0.03));
  return viewport - 2 * margin;
}

test("the folder table turns into cards between an 800px and a 700px window", () => {
  expect(css("src/theme/workspace.css")).toContain(
    "padding: 0 clamp(14px, 3vw, 36px) 24px;",
  );
  const browse = css("src/theme/browse-grid.css");
  const cards = Number(
    /@container \(max-width: (\d+)px\) \{\s*\.du-browse-grid__header \{/.exec(
      browse,
    )?.[1],
  );
  expect(listWidth(700)).toBeLessThanOrEqual(cards);
  expect(listWidth(800)).toBeGreaterThan(cards);
  // A vertical scroll bar of up to 17px must not change either outcome.
  expect(listWidth(800) - 17).toBeGreaterThan(cards);
  // The column minimums stay as the guard for zoom and long text.
  expect(browse).toContain(
    "minmax(0, 32fr) minmax(4em, 8fr) repeat(2, minmax(8em, 15fr))\n    minmax(10em, 16fr) minmax(0, 14fr);",
  );
  // The other users of the same switch move with it.
  expect(BROWSE_CARD_WIDTH).toBe(cards);
  expect(css("src/features/groups/groups.css")).toContain(
    "@container (min-width: " + (cards + 1) + "px)",
  );
});
