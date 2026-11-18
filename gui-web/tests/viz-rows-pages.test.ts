import { describe, expect, test, vi } from "vitest";
import { MAX_ROWS_PER_READ, readRowsPages } from "../src/viz";
import type { VizRow, VizRows, VizRowsRequest } from "../src/ipc";
import { modelFixture } from "../preview/model-fixture";

const row = (n: number): VizRow => ({
  ...modelFixture().page.response.rows[0]!,
  itemRef: { resultVersion: "r", itemNo: n },
});
function source(
  total: number,
  patch: (page: VizRows, call: number) => VizRows = (p) => p,
) {
  let call = 0;
  return vi.fn(async (request: VizRowsRequest): Promise<VizRows> => {
    const end = Math.min(request.start + request.count, total);
    const rows = [];
    for (let n = request.start; n < end; n += 1) rows.push(row(n));
    return patch(
      {
        rows,
        totalCount: total,
        resultRevision: "r",
        observedAt: "2026-11-15T00:00:00.000Z",
        completeness: "complete",
      },
      call++,
    );
  });
}
const request = (count: number): VizRowsRequest => ({
  modelRef: "m",
  resultRevision: "r",
  start: 0,
  count,
});

describe("F06 FR7.HD1.gui-web.DD1: reading rows past the page limit", () => {
  test("a count within the limit is one call, unchanged", async () => {
    const read = source(50);
    const joined = await readRowsPages(read, request(100));
    expect(read).toHaveBeenCalledTimes(1);
    expect(read).toHaveBeenCalledWith(request(100));
    expect(joined.rows).toHaveLength(50);
  });

  test("a larger count is read in pages of at most the limit and joined in order", async () => {
    const read = source(2300);
    const joined = await readRowsPages(read, request(2500));
    expect(read.mock.calls.map(([r]) => [r.start, r.count])).toEqual([
      [0, MAX_ROWS_PER_READ],
      [1000, MAX_ROWS_PER_READ],
      [2000, 500],
    ]);
    expect(joined.rows.map((r) => r.itemRef?.itemNo)).toEqual(
      Array.from({ length: 2300 }, (_, n) => n),
    );
    expect(joined.totalCount).toBe(2300);
  });

  test("the read stops at the requested count and at the end of the rows", async () => {
    const read = source(5000);
    expect((await readRowsPages(read, request(1500))).rows).toHaveLength(1500);
    expect(read.mock.calls.map(([r]) => [r.start, r.count])).toEqual([
      [0, 1000],
      [1000, 500],
    ]);
    const short = source(1000);
    await readRowsPages(short, request(2000));
    expect(short).toHaveBeenCalledTimes(1);
  });

  test.each([
    ["observation", { observedAt: "2026-11-15T00:00:01.000Z" }],
    ["completeness", { completeness: "partial" as const }],
    ["revision", { resultRevision: "other" }],
    ["total", { totalCount: 4000 }],
  ])("a page with another %s fails the whole read", async (_name, change) => {
    const read = source(2300, (page, call) =>
      call === 0 ? page : { ...page, ...change },
    );
    await expect(readRowsPages(read, request(2300))).rejects.toThrow(
      "Rows changed between pages",
    );
  });

  test("an empty page ends the read instead of looping", async () => {
    const read = source(2300, (page, call) =>
      call === 0 ? page : { ...page, rows: [] },
    );
    const joined = await readRowsPages(read, request(2300));
    expect(joined.rows).toHaveLength(1000);
    expect(read).toHaveBeenCalledTimes(2);
  });
});
