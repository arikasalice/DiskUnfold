import { expect, it } from "vitest";
import { BrowseRowPositions } from "../src/components/browse-row-positions";

it("seeks within one million absolute positions using sparse measured heights", () => {
  const sizes = new Map([
    [0, 192],
    [999_999, 32],
  ]);
  const rows = new BrowseRowPositions(1_000_000, 64, sizes);
  expect(rows.offset(1)).toBe(192);
  expect(rows.offset(999_999)).toBe(64_000_064);
  expect(rows.height).toBe(64_000_096);
  expect(rows.at(191)).toBe(0);
  expect(rows.at(192)).toBe(1);
  expect(rows.at(rows.height - 1)).toBe(999_999);
  expect(rows.at(rows.height)).toBe(1_000_000);
});

it("counts off-screen focus pins inside the twenty extra row budget", () => {
  const rows = new BrowseRowPositions(1_000_000, 64, new Map());
  const window = rows.window(64 * 100, 64 * 106, [0, 999_999, 0]);
  expect(window.visibleStart).toBe(100);
  expect(window.visibleEnd).toBe(106);
  expect(window.indexes).toHaveLength(26);
  expect(window.indexes).toEqual(
    expect.arrayContaining([0, 100, 101, 102, 103, 104, 105, 999_999]),
  );
  expect(window.indexes).not.toContain(89);
  expect(window.indexes).not.toContain(116);
});

it("supports short lists and excludes a row exactly beyond the viewport", () => {
  expect(new BrowseRowPositions(0, 64, new Map()).window(0, 100, [0])).toEqual({
    visibleStart: 0,
    visibleEnd: 0,
    indexes: [],
  });
  expect(new BrowseRowPositions(2, 64, new Map()).window(-10, 64, [])).toEqual({
    visibleStart: 0,
    visibleEnd: 1,
    indexes: [0, 1],
  });
});
