import { describe, expect, it } from "vitest";
import type { FolderQueryPage } from "../src/ipc";
import {
  browsePageStart,
  BrowsePages,
  visibleBrowsePages,
} from "../src/state/browse-pages";

function page(count: number, start = 0): FolderQueryPage {
  return {
    rowKind: "folder",
    queryRef: "query",
    resultVersion: "version",
    rows: Array.from({ length: Math.min(200, count - start) }, (_, i) => ({
      item: { resultVersion: "version", itemNo: start + i },
      displayName: { state: "absent" },
      kind: "file",
      logicalSize: { state: "absent" },
      allocatedSize: { state: "absent" },
      modifiedAt: { state: "absent" },
      hasChildFolder: { state: "absent" },
      flags: {
        excluded: { state: "absent" },
        unenumerated: { state: "absent" },
      },
      correspondence: { state: "absent" },
    })),
    total: { state: "exact", count },
    unknownCount: 0,
    observedAt: null,
    completeness: "complete",
    nextCursor: null,
  };
}

describe("fixed query pages", () => {
  it.each([0, 1, 200, 201, 2001, 1_000_000])(
    "uses absolute positions and header-aware metadata for %i rows",
    (count) => {
      const pages = new BrowsePages(page(count));
      expect(pages.rowCount).toBe(count + 1);
      expect(visibleBrowsePages(0, Math.min(12, count), count)).toEqual(
        count === 0 ? [] : count <= 200 ? [0] : [0, 200],
      );
      if (count === 0) return;
      const start = browsePageStart(count - 1);
      expect(pages.request(start)).toEqual({
        queryRef: "query",
        resultVersion: "version",
        start,
        count: 200,
      });
      expect(pages.accept(start, page(count, start), new Set())).toBe(true);
      expect(pages.row(count - 1)?.item.itemNo).toBe(count - 1);
      expect(pages.row(count)).toBeUndefined();
    },
  );

  it("prefetches after the last visible page and stops at the known count", () => {
    expect(visibleBrowsePages(199, 201, 1000)).toEqual([0, 200, 400]);
    expect(visibleBrowsePages(399, 401, 401)).toEqual([200, 400]);
  });

  it("keeps lower-bound counts unknown to assistive technology", () => {
    const first = page(201);
    const last = page(201, 200);
    first.total.state = last.total.state = "lower_bound";
    const pages = new BrowsePages(first);
    expect(pages.rowCount).toBe(-1);
    expect(pages.accept(200, last, new Set())).toBe(true);
    expect(pages.row(200)?.item.itemNo).toBe(200);
  });

  it("rejects cross-query, cross-version, short, and duplicate pages", () => {
    const pages = new BrowsePages(page(201));
    const last = page(201, 200);
    for (const invalid of [
      { ...last, queryRef: "other" },
      { ...last, resultVersion: "other" },
      { ...last, rows: [] },
      { ...last, rows: page(1).rows },
    ]) {
      expect(pages.accept(200, invalid, new Set())).toBe(false);
      expect(pages.has(200)).toBe(false);
      expect(pages.row(0)?.item.itemNo).toBe(0);
    }
  });

  it("rejects malformed first pages and changes to cached absolute row identities", () => {
    const first = page(201);
    expect(
      () =>
        new BrowsePages({
          ...first,
          rowKind: "search",
        } as unknown as FolderQueryPage),
    ).toThrow();
    expect(
      () => new BrowsePages({ ...first, rows: first.rows.slice(1) }),
    ).toThrow();
    expect(
      () =>
        new BrowsePages({
          ...first,
          rows: first.rows.map((row) => ({
            ...row,
            item: { ...row.item, resultVersion: "other" },
          })),
        }),
    ).toThrow();
    expect(
      () =>
        new BrowsePages({
          ...first,
          rows: first.rows.map(() => first.rows[0]!),
        }),
    ).toThrow();
    const cache = new BrowsePages(first);
    expect(
      cache.accept(0, { ...first, rows: [...first.rows].reverse() }, new Set()),
    ).toBe(false);
    expect(cache.row(0)?.item.itemNo).toBe(0);
  });

  it("evicts the least recently used unprotected page at the ten-page limit", () => {
    const pages = new BrowsePages(page(2400));
    const focus = new Set([0]);
    for (let start = 200; start < 2000; start += 200) {
      expect(pages.accept(start, page(2400, start), focus)).toBe(true);
    }
    pages.touch(200);
    expect(pages.accept(2000, page(2400, 2000), focus)).toBe(true);
    expect(pages.has(0)).toBe(true);
    expect(pages.has(200)).toBe(true);
    expect(pages.has(400)).toBe(false);
    expect(
      Array.from({ length: 12 }, (_, i) => pages.has(i * 200)).filter(Boolean),
    ).toHaveLength(10);
  });
});
