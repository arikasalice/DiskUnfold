import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { HistoryPanel } from "../src/features/history/HistoryPanel";
import { AnalysisError } from "../src/ipc";
import type {
  AnalysisClient,
  EventEnvelope,
  HistoryDetail,
  HistoryListRow,
} from "../src/ipc";

afterEach(cleanup);
const row = (jobRef: string): HistoryListRow => ({
  jobRef,
  missingReasons: [],
  summary: {
    kindId: "scan.metadata",
    kindVersion: 1,
    scopeSummary: "One folder",
    state: "done",
    counts: null,
    startedAtMs: null,
    finishedAtMs: null,
    predecessorJobRef: null,
    relation: null,
    sessionRestoreAvailable: null,
  },
});
const detail: HistoryDetail = {
  summary: null,
  workflow: { planVersions: null, executedPlan: null, approvalCheck: null },
  followUps: null,
  effects: { estimated: null, executed: null, confirmed: null },
  completeness: "partial",
  missingReasons: ["read_failed"],
};
const page = {
  revision: { deleteGeneration: "0", maxRecordSeq: "2" },
  rows: [row("first"), row("second")],
  nextCursor: null,
};
function client(call: ReturnType<typeof vi.fn>): AnalysisClient {
  return { call, subscribe: () => () => {} } as unknown as AnalysisClient;
}

test("opens a valid row, preserves filters and scroll, and restores focus", async () => {
  const call = vi.fn((name: string) =>
    Promise.resolve(name === "history.list" ? page : detail),
  );
  render(<HistoryPanel client={client(call)} onClose={() => {}} />);
  const state = await screen.findByLabelText("End state");
  fireEvent.change(state, { target: { value: "failed" } });
  await waitFor(() =>
    expect(
      call.mock.calls.filter(([name]) => name === "history.list").length,
    ).toBe(2),
  );
  const region = await screen.findByRole("region", {
    name: "Operation history scroll area",
  });
  Object.defineProperty(region, "scrollTop", {
    value: 100,
    writable: true,
    configurable: true,
  });
  fireEvent.scroll(region);
  const button = screen.getAllByRole("button", { name: /^View details:/ })[0];
  if (!button) throw new Error("missing detail button");
  fireEvent.click(button);
  await screen.findByText("Operation summary unavailable.");
  expect(
    call.mock.calls.filter(([name]) => name === "history.get"),
  ).toHaveLength(1);
  expect(screen.getAllByText("Unavailable").length).toBeGreaterThanOrEqual(6);
  expect(screen.getByText("Follow-up information unavailable.")).toBeDefined();
  expect(screen.queryByText("No follow-up operations.")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Back to history" }));
  await waitFor(() => expect(document.activeElement).toBe(button));
  expect((screen.getByLabelText("End state") as HTMLSelectElement).value).toBe(
    "failed",
  );
  expect(region.scrollTop).toBe(100);
});

test("only rows with a job reference expose an action named by operation and time", async () => {
  const valid = row("raw-secret-reference");
  const invalid = { ...row("unused"), jobRef: null };
  const call = vi.fn().mockResolvedValue({ ...page, rows: [valid, invalid] });
  render(<HistoryPanel client={client(call)} onClose={() => {}} />);
  const actions = await screen.findAllByRole("button", {
    name: /^View details:/,
  });
  expect(actions).toHaveLength(1);
  expect(actions[0]?.getAttribute("aria-label")).toContain(
    "Disk analysis, Time unavailable",
  );
  expect(actions[0]?.getAttribute("aria-label")).not.toContain(
    "raw-secret-reference",
  );
});

test("removed history has its own message and storage failures can be retried once", async () => {
  const call = vi
    .fn()
    .mockImplementationOnce(() => Promise.resolve(page))
    .mockRejectedValueOnce(new AnalysisError("NOT_FOUND", "history.removed"));
  const view = render(
    <HistoryPanel client={client(call)} onClose={() => {}} />,
  );
  fireEvent.click(
    (await screen.findAllByRole("button", { name: /^View details:/ }))[0]!,
  );
  await screen.findByText("History was removed and can no longer be viewed.");
  expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  view.unmount();
  let resolveDetail: ((value: HistoryDetail) => void) | undefined;
  const retry = vi
    .fn()
    .mockImplementationOnce(() => Promise.resolve(page))
    .mockRejectedValueOnce(new Error("storage"))
    .mockImplementationOnce(
      () =>
        new Promise<HistoryDetail>((resolve) => {
          resolveDetail = resolve;
        }),
    );
  render(<HistoryPanel client={client(retry)} onClose={() => {}} />);
  fireEvent.click(
    (await screen.findAllByRole("button", { name: /^View details:/ }))[0]!,
  );
  const button = await screen.findByRole("button", { name: "Retry" });
  fireEvent.click(button);
  fireEvent.click(button);
  expect(
    retry.mock.calls.filter(([name]) => name === "history.get"),
  ).toHaveLength(2);
  await act(async () => {
    resolveDetail?.(detail);
  });
  await screen.findByText("Operation summary unavailable.");
});

test("late detail response cannot overwrite a later selection", async () => {
  let finish: ((value: HistoryDetail) => void) | undefined;
  const call = vi
    .fn()
    .mockImplementationOnce(() => Promise.resolve(page))
    .mockImplementationOnce(
      () =>
        new Promise<HistoryDetail>((resolve) => {
          finish = resolve;
        }),
    )
    .mockResolvedValueOnce({ ...detail, followUps: [] });
  render(<HistoryPanel client={client(call)} onClose={() => {}} />);
  fireEvent.click(
    (await screen.findAllByRole("button", { name: /^View details:/ }))[0]!,
  );
  fireEvent.click(screen.getByRole("button", { name: "Back to history" }));
  fireEvent.click(
    screen.getAllByRole("button", { name: /^View details:/ })[1]!,
  );
  await screen.findByText("No follow-up operations.");
  await act(async () => {
    finish?.(detail);
  });
  expect(screen.getByText("No follow-up operations.")).toBeDefined();
  expect(
    within(screen.getByLabelText("Effects")).getAllByText("Unavailable"),
  ).toHaveLength(3);
});

test("renders supplied plans, approval, follow-ups and the three distinct effects", async () => {
  const populated: HistoryDetail = {
    ...detail,
    workflow: {
      planVersions: ["plan-a", "plan-b"],
      executedPlan: "plan-b",
      approvalCheck: "matched",
    },
    followUps: [
      {
        jobRef: "follow-up",
        relation: "retry",
        state: "failed",
        finishedAtMs: null,
      },
    ],
    effects: {
      estimated: { value: "10", unit: "bytes", uncertainty: "estimated" },
      executed: { value: "4", unit: "bytes", uncertainty: "exact" },
      confirmed: { value: "2", unit: "bytes", uncertainty: "measured" },
    },
  };
  const call = vi
    .fn()
    .mockResolvedValueOnce(page)
    .mockResolvedValueOnce(populated);
  const next = row("follow-up").summary;
  if (!next) throw new Error("missing follow-up summary fixture");
  call
    .mockResolvedValueOnce({
      ...detail,
      summary: { ...next, predecessorJobRef: "first" },
    })
    .mockResolvedValueOnce(populated);
  render(<HistoryPanel client={client(call)} onClose={() => {}} />);
  fireEvent.click(
    (await screen.findAllByRole("button", { name: /^View details:/ }))[0]!,
  );
  const plans = await screen.findByLabelText("Plans and approval");
  expect(within(plans).getByText("plan-a, plan-b")).toBeDefined();
  expect(within(plans).getByText("matched")).toBeDefined();
  const effects = screen.getByLabelText("Effects");
  expect(within(effects).getByText("10 bytes · estimated")).toBeDefined();
  expect(within(effects).getByText("4 bytes · exact")).toBeDefined();
  expect(within(effects).getByText("2 bytes · measured")).toBeDefined();
  expect(screen.getByText("Retry · Failed · Time unavailable")).toBeDefined();
  expect(screen.queryByText("follow-up")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Open follow-up 1" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Open previous operation" }),
  );
  await screen.findByText("Retry · Failed · Time unavailable");
  expect(
    call.mock.calls.filter(([command]) => command === "history.get"),
  ).toEqual([
    ["history.get", { jobRef: "first" }],
    ["history.get", { jobRef: "follow-up" }],
    ["history.get", { jobRef: "first" }],
  ]);
});

test("returns focus to the list control when refreshed history is empty", async () => {
  let listener: ((event: EventEnvelope) => void) | undefined;
  const call = vi.fn((name: string) =>
    Promise.resolve(
      name === "history.get"
        ? detail
        : call.mock.calls.filter(([cmd]) => cmd === "history.list").length > 1
          ? { ...page, rows: [] }
          : page,
    ),
  );
  const api = {
    call,
    subscribe: (fn: (event: EventEnvelope) => void) => {
      listener = fn;
      return () => {};
    },
  } as unknown as AnalysisClient;
  render(<HistoryPanel client={api} onClose={() => {}} />);
  fireEvent.click(
    (await screen.findAllByRole("button", { name: /^View details:/ }))[0]!,
  );
  await screen.findByText("Operation summary unavailable.");
  act(() => {
    listener?.({
      topic: "job.resync_required",
      contractVersion: 1,
      seq: 1,
      occurredAt: "2026-11-10T00:00:00Z",
      body: {},
    });
  });
  await waitFor(() =>
    expect(
      call.mock.calls.filter(([cmd]) => cmd === "history.list"),
    ).toHaveLength(2),
  );
  fireEvent.click(screen.getByRole("button", { name: "Back to history" }));
  await screen.findByText("No history matches these filters.");
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Back to analysis" }),
    ),
  );
});
