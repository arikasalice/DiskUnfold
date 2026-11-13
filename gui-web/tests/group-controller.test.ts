import { afterEach, expect, test, vi } from "vitest";
import { GroupController } from "../src/features/visualization/group-controller";
import type { GroupFrame } from "../src/features/visualization/group-controller";
import { AnalysisError } from "../src/ipc";
import type { VizGroupRows, VizGroupRowsRequest } from "../src/ipc";

const frame: GroupFrame = {
  modelRef: "model-a",
  resultRevision: "rev-a",
  observedAt: "2026-11-12T00:00:00Z",
  completeness: "complete",
};
const size = {
  basis: "logical",
  evidence: "measured",
  state: "exact",
  bytes: "10",
} as const;
const range = { kind: "known", min: size, max: size } as const;
function response(
  request: VizGroupRowsRequest,
  overrides: Partial<VizGroupRows> = {},
): VizGroupRows {
  return {
    rows: Array.from(
      { length: Math.min(request.count, 3 - request.start) },
      (_, index) => ({
        itemRef: { resultVersion: "rev-a", itemNo: request.start + index + 1 },
        groupRef: null,
        displayName: `item-${request.start + index + 1}`,
        size,
        ratio: null,
        denominator: { basis: "scope", value: "3" },
        legendClassKey: "file",
        displayFlags: [],
      }),
    ),
    totalCount: 3,
    groupTotal: size,
    sizeRange: range,
    resultRevision: "rev-a",
    observedAt: frame.observedAt,
    completeness: frame.completeness,
    ...overrides,
  } as VizGroupRows;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
const selection = { groupRef: "group-a", shape: { groupRef: "group-a", size } };
afterEach(() => vi.restoreAllMocks());

test("validates evidence and page bounds, and retains last confirmed page on retryable errors", async () => {
  const rows = vi.fn(async (request: VizGroupRowsRequest) =>
    response(request, { totalCount: 5 }),
  );
  const controller = new GroupController({ rows });
  expect(
    await controller.open(frame, {
      ...selection,
      shape: { ...selection.shape, groupRef: "wrong" },
    }),
  ).toBe(false);
  expect(rows).not.toHaveBeenCalled();
  expect(await controller.open(frame, selection, 2)).toBe(true);
  expect(controller.getSnapshot()).toMatchObject({
    status: "ready",
    rangeLabel: "1–2 / 5",
    hasNext: true,
  });
  rows.mockRejectedValueOnce(new Error("temporary"));
  expect(await controller.next()).toBe(false);
  expect(controller.getSnapshot()).toMatchObject({
    status: "error",
    retryStart: 2,
    canRetry: true,
    page: { request: { start: 0 } },
    rangeLabel: "1–2 / 5",
  });
  rows.mockImplementationOnce(async (request) =>
    response(request, { totalCount: 5 }),
  );
  expect(await controller.retry()).toBe(true);
  expect(rows.mock.calls.at(-1)?.[0].start).toBe(2);
  expect(controller.getSnapshot()).toMatchObject({
    status: "ready",
    rangeLabel: "3–3 / 5",
  });
});

test("legacy AnalysisError construction keeps the retryable fallback", async () => {
  const controller = new GroupController({
    rows: vi
      .fn()
      .mockRejectedValue(
        new AnalysisError("INTERNAL", "support.internal_error"),
      ),
  });

  expect(await controller.open(frame, selection)).toBe(false);
  expect(controller.getSnapshot()).toMatchObject({
    status: "error",
    failure: "page",
    canRetry: true,
    retryStart: 0,
  });
});

test("rejects response metadata and group totals that drift between pages", async () => {
  const rows = vi.fn(async (request: VizGroupRowsRequest) => response(request));
  const controller = new GroupController({ rows });
  await controller.open(frame, selection, 2);
  rows.mockResolvedValueOnce(
    response(
      { modelRef: "model-a", groupRef: "group-a", start: 2, count: 2 },
      {
        groupTotal: { ...size, bytes: "11" },
      },
    ),
  );
  expect(await controller.next()).toBe(false);
  expect(controller.getSnapshot()).toMatchObject({
    status: "error",
    failure: "invalid",
    canRetry: false,
    page: null,
    rangeLabel: undefined,
  });
  expect(await controller.open(frame, selection, 2)).toBe(true);
  rows.mockResolvedValueOnce(
    response(
      { modelRef: "model-a", groupRef: "group-a", start: 2, count: 2 },
      {
        resultRevision: "rev-other",
      },
    ),
  );
  expect(await controller.next()).toBe(false);
});

test("advances by received row count and withdraws cached rows on stale version", async () => {
  const rows = vi.fn(async (request: VizGroupRowsRequest) => {
    const data = response(request, { totalCount: 4 });
    if (request.start === 0) data.rows = data.rows.slice(0, 1);
    return data;
  });
  const controller = new GroupController({ rows });
  await controller.open(frame, selection, 2);
  expect(controller.getSnapshot().hasNext).toBe(true);
  rows.mockRejectedValueOnce({ code: "STALE_VERSION" });
  expect(await controller.next()).toBe(false);
  expect(rows.mock.calls.at(-1)?.[0].start).toBe(1);
  expect(controller.getSnapshot()).toMatchObject({
    status: "error",
    failure: "stale",
    page: null,
    rangeLabel: undefined,
    hasNext: false,
    hasPrevious: false,
    canRetry: false,
  });
});

test("ignores late responses after close and after frame replacement, including A to B to A", async () => {
  const a = deferred<VizGroupRows>();
  const b = deferred<VizGroupRows>();
  const rows = vi
    .fn()
    .mockReturnValueOnce(a.promise)
    .mockReturnValueOnce(b.promise);
  const controller = new GroupController({ rows });
  const opening = controller.open(frame, selection);
  controller.close();
  a.resolve(
    response({
      modelRef: "model-a",
      groupRef: "group-a",
      start: 0,
      count: 100,
    }),
  );
  expect(await opening).toBe(false);
  expect(controller.getSnapshot()).toMatchObject({
    status: "closed",
    page: null,
  });

  const first = controller.open(frame, selection);
  controller.replaceFrame({ ...frame, modelRef: "model-b" });
  controller.replaceFrame(frame);
  b.resolve(
    response({
      modelRef: "model-a",
      groupRef: "group-a",
      start: 0,
      count: 100,
    }),
  );
  expect(await first).toBe(false);
  expect(controller.getSnapshot()).toMatchObject({
    frame: { modelRef: "model-a" },
    groupRef: null,
    page: null,
    status: "closed",
    rangeLabel: undefined,
  });
});

test.each([
  ["STALE_VERSION", "stale", false],
  ["NOT_FOUND", "missing", false],
  ["INVALID_ARGUMENT", "invalid", false],
  ["OTHER", "page", true],
] as const)("classifies %s distinctly", async (code, failure, canRetry) => {
  const rows = vi.fn().mockRejectedValue({ code });
  const controller = new GroupController({ rows });
  expect(await controller.open(frame, selection)).toBe(false);
  expect(controller.getSnapshot()).toMatchObject({
    status: "error",
    failure,
    canRetry,
  });
});

test("short pages return to the actual previous start and accept group rows", async () => {
  const rows = vi.fn(async (request: VizGroupRowsRequest) => {
    const result = response(request);
    if (request.start === 0) result.rows = result.rows.slice(0, 1);
    if (request.start === 1)
      result.rows[0] = {
        ...result.rows[0]!,
        itemRef: null,
        groupRef: "nested",
      };
    return result;
  });
  const controller = new GroupController({ rows });
  expect(await controller.open(frame, selection, 2)).toBe(true);
  expect(await controller.next()).toBe(true);
  expect(controller.getSnapshot().page?.request.start).toBe(1);
  expect(await controller.next()).toBe(false);
  expect(await controller.previous()).toBe(true);
  expect(controller.getSnapshot().page?.request.start).toBe(0);
});
