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
import { App } from "../src/app/App";
import { AnalysisError } from "../src/ipc";
import type {
  AnalysisClient,
  EventEnvelope,
  HistoryListResponse,
  HistoryListRevision,
} from "../src/ipc";

afterEach(cleanup);

const response = (
  rows: HistoryListResponse["rows"] = [],
  nextCursor: string | null = null,
): HistoryListResponse => ({
  revision: { deleteGeneration: "0", maxRecordSeq: "1" },
  rows,
  nextCursor,
});
const row = {
  jobRef: "one",
  missingReasons: [],
  summary: {
    kindId: "scan.metadata",
    kindVersion: 1,
    scopeSummary: "1 folder",
    state: "done",
    counts: null,
    startedAtMs: null,
    finishedAtMs: null,
    predecessorJobRef: null,
    relation: null,
    sessionRestoreAvailable: null,
  },
} satisfies HistoryListResponse["rows"][number];

test("loads history, filters, and preserves opaque paging cursors", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce(response([row], "opaque"))
    .mockResolvedValueOnce(response([row]))
    .mockResolvedValue(response());
  const client = {
    call,
    subscribe: () => () => {},
  } as unknown as AnalysisClient;
  render(<HistoryPanel client={client} onClose={() => {}} />);
  expect(
    await screen.findByRole("table", { name: "Operation history" }),
  ).toBeDefined();
  expect(screen.getByText("Time unavailable")).toBeDefined();
  expect(screen.getByText("1 folder")).toBeDefined();
  expect(
    within(screen.getByRole("table", { name: "Operation history" })).getByText(
      "Completed",
    ),
  ).toBeDefined();
  expect(screen.queryByLabelText("Result counts")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Load more history" }));
  await waitFor(() => expect(call).toHaveBeenCalledTimes(2));
  expect(call.mock.calls[1]?.[1]).toMatchObject({ cursor: "opaque" });
  fireEvent.change(screen.getByLabelText("End state"), {
    target: { value: "failed" },
  });
  await screen.findByText("No history matches these filters.");
  expect(call.mock.calls[2]?.[1]).toMatchObject({
    cursor: null,
    filter: { states: ["failed"] },
  });
});

test("ignores a late response when filters change", async () => {
  let finish: ((value: HistoryListResponse) => void) | undefined;
  const call = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise<HistoryListResponse>((resolve) => {
          finish = resolve;
        }),
    )
    .mockResolvedValue(response());
  const client = {
    call,
    subscribe: () => () => {},
  } as unknown as AnalysisClient;
  render(<HistoryPanel client={client} onClose={() => {}} />);
  expect(
    screen.getByRole("progressbar", { name: "Loading history" }),
  ).toBeDefined();
  fireEvent.change(screen.getByLabelText("End state"), {
    target: { value: "failed" },
  });
  await screen.findByText("No history matches these filters.");
  await act(async () => {
    finish?.(response([row]));
  });
  expect(screen.queryByRole("table", { name: "Operation history" })).toBeNull();
});

test("reveals precise result counts only in the selected record", async () => {
  const maximum = "9223372036854775807";
  const sample = {
    ...row,
    summary: {
      ...row.summary,
      counts: {
        completed: maximum,
        overwritten: "0",
        failed: "1",
        skipped: "2",
        unprocessed: "3",
      },
    },
  };
  const call = vi
    .fn()
    .mockResolvedValueOnce(response([sample]))
    .mockResolvedValueOnce({
      summary: sample.summary,
      workflow: { planVersions: [], executedPlan: null, approvalCheck: null },
      followUps: [],
      effects: { estimated: null, executed: null, confirmed: null },
      completeness: "complete",
      missingReasons: [],
    });
  const client = historyEvents(call).client;
  render(<HistoryPanel client={client} onClose={() => {}} />);
  const open = await screen.findByRole("button", { name: /^View details:/ });
  expect(screen.queryByText(maximum)).toBeNull();
  fireEvent.click(open);
  const evidence = await screen.findByRole("button", {
    name: "Recorded evidence",
  });
  expect(evidence.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByText(maximum)).toBeNull();
  fireEvent.click(evidence);
  const counts = within(await screen.findByLabelText("Result counts"));
  for (const value of [maximum, "0", "1", "2", "3"])
    expect(counts.getByText(value)).toBeDefined();
  expect(screen.queryByText("Not yet confirmed")).toBeNull();
  expect(
    within(screen.getByLabelText("Effects")).getAllByText("Unavailable"),
  ).toHaveLength(3);
});

test("restarts a stale page", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce(response([row], "opaque"))
    .mockRejectedValueOnce(
      new AnalysisError("STALE_VERSION", "history.version_changed"),
    )
    .mockResolvedValueOnce(response());
  const client = {
    call,
    subscribe: () => () => {},
  } as unknown as AnalysisClient;
  render(<HistoryPanel client={client} onClose={() => {}} />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Load more history" }),
  );
  await screen.findByText("History changed. Showing the first page again.");
  await screen.findByText("No operations recorded yet.");
  expect(call.mock.calls[2]?.[1]).toMatchObject({ cursor: null });
});

function historyEvents(call: ReturnType<typeof vi.fn>) {
  let listen: ((event: EventEnvelope) => void) | undefined;
  let seq = 0;
  const client = {
    call,
    subscribe(listener: (event: EventEnvelope) => void) {
      listen = listener;
      return () => {
        listen = undefined;
      };
    },
  } as unknown as AnalysisClient;
  return {
    client,
    send(topic: string, revision?: HistoryListRevision) {
      listen?.({
        topic,
        contractVersion: 1,
        seq: ++seq,
        occurredAt: "1970-01-01T00:00:00.000Z",
        body: { revision },
      });
    },
  };
}

test("refreshes a new committed revision once and discards an obsolete page", async () => {
  const revision = { deleteGeneration: "0", maxRecordSeq: "9007199254740992" };
  const newer = { deleteGeneration: "0", maxRecordSeq: "9007199254740993" };
  let finish: ((page: HistoryListResponse) => void) | undefined;
  const call = vi
    .fn()
    .mockResolvedValueOnce({ ...response([row], "old"), revision })
    .mockImplementationOnce(
      () =>
        new Promise<HistoryListResponse>((resolve) => {
          finish = resolve;
        }),
    )
    .mockResolvedValueOnce({ ...response(), revision: newer });
  const events = historyEvents(call);
  render(<HistoryPanel client={events.client} onClose={() => {}} />);
  const more = await screen.findByRole("button", { name: "Load more history" });
  act(() => {
    events.send("history.changed", revision);
    events.send("history.changed", { ...revision, maxRecordSeq: "invalid" });
  });
  fireEvent.click(more);
  expect(call).toHaveBeenCalledTimes(2);
  act(() => {
    events.send("history.changed", newer);
    events.send("history.changed", newer);
    events.send("history.changed", revision);
  });
  await screen.findByText("No operations recorded yet.");
  expect(call.mock.calls[2]?.[1]).toMatchObject({ cursor: null });
  await act(async () => {
    finish?.({ ...response([row]), revision });
  });
  expect(screen.queryByRole("table", { name: "Operation history" })).toBeNull();
  act(() => events.send("history.changed", newer));
  expect(call).toHaveBeenCalledTimes(3);
});

test("resynchronizes history after subscriber overflow", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce(response([row], "old"))
    .mockResolvedValueOnce(response());
  const events = historyEvents(call);
  const { unmount } = render(
    <HistoryPanel client={events.client} onClose={() => {}} />,
  );
  await screen.findByRole("table", { name: "Operation history" });
  act(() => events.send("job.resync_required"));
  await screen.findByText("No operations recorded yet.");
  expect(call).toHaveBeenCalledTimes(2);
  expect(call.mock.calls[1]?.[1]).toMatchObject({ cursor: null });
  unmount();
  act(() => events.send("job.resync_required"));
  expect(call).toHaveBeenCalledTimes(2);
});

test("announces an error and retries the first page", async () => {
  const call = vi
    .fn()
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValue(response());
  const client = {
    call,
    subscribe: () => () => {},
  } as unknown as AnalysisClient;
  render(<HistoryPanel client={client} onClose={() => {}} />);
  expect(await screen.findByRole("alert")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  await screen.findByText("No operations recorded yet.");
  expect(call.mock.calls[1]?.[1]).toMatchObject({ cursor: null });
});

test("history is a separate screen and closing returns focus", async () => {
  const call = vi.fn((name: string) =>
    name === "history.list"
      ? Promise.resolve(response())
      : new Promise(() => {}),
  );
  const client = {
    call,
    subscribe: () => () => {},
  } as unknown as AnalysisClient;
  render(<App analysis={client} />);
  const open = screen.getByRole("button", { name: "Operation history" });
  open.focus();
  fireEvent.keyDown(open, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(open, { key: "Enter", code: "Enter" });
  await screen.findByText("No operations recorded yet.");
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Back to analysis" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Back to analysis" }));
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Operation history" }),
    ),
  );
});

test("keeps secondary filters collapsed and preserves their active state", async () => {
  const call = vi.fn().mockResolvedValue(response());
  render(
    <HistoryPanel client={historyEvents(call).client} onClose={() => {}} />,
  );
  await screen.findByText("No operations recorded yet.");
  expect(screen.queryByLabelText("From date")).toBeNull();
  const extra = screen.getByRole("button", { name: "Date and type filters" });
  extra.focus();
  fireEvent.keyDown(extra, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(extra, { key: "Enter", code: "Enter" });
  fireEvent.change(screen.getByLabelText("From date"), {
    target: { value: "2026-11-10" },
  });
  fireEvent.change(screen.getByLabelText("Through date"), {
    target: { value: "2026-11-09" },
  });
  await screen.findByText("Choose a valid date range.");
  fireEvent.change(screen.getByLabelText("Through date"), {
    target: { value: "2026-11-10" },
  });
  await screen.findByText("No history matches these filters.");
  expect(call.mock.lastCall?.[1]).toMatchObject({
    filter: {
      finishedFromMs: new Date("2026-11-10T00:00:00").getTime(),
      finishedBeforeMs: new Date("2026-11-11T00:00:00").getTime(),
    },
  });
  fireEvent.click(
    screen.getByRole("button", { name: "Date and type filters (active)" }),
  );
  expect(screen.queryByLabelText("From date")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
  await screen.findByText("No operations recorded yet.");
  expect(document.activeElement).toBe(screen.getByLabelText("End state"));
  expect(call.mock.lastCall?.[1]).toMatchObject({
    filter: {
      finishedFromMs: null,
      finishedBeforeMs: null,
      kinds: [],
      states: [],
    },
  });
});

test("virtualizes a long history and keeps the loaded count outside the scrolling rows", async () => {
  const rows = Array.from({ length: 80 }, (_, index) => ({
    ...row,
    jobRef: "job-" + index,
    summary: { ...row.summary, scopeSummary: "Recorded target " + (index + 1) },
  }));
  const call = vi.fn().mockResolvedValue(response(rows));
  render(
    <HistoryPanel client={historyEvents(call).client} onClose={() => {}} />,
  );
  const table = await screen.findByRole("table", { name: "Operation history" });
  expect(within(table).getAllByRole("row").length).toBeLessThan(40);
  expect(screen.queryByText("Recorded target 80")).toBeNull();
  const region = screen.getByRole("region", {
    name: "Operation history scroll area",
  });
  Object.defineProperty(region, "scrollTop", {
    value: 80 * 64,
    configurable: true,
  });
  fireEvent.scroll(region);
  expect(screen.getByText("Recorded target 80")).toBeDefined();
  expect(screen.getByText("80 records shown")).toBeDefined();
  expect(table.getAttribute("aria-rowcount")).toBe("81");
});

test("moves keyboard focus to the history viewport after the last page", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce(response([row], "next"))
    .mockResolvedValueOnce(response([{ ...row, jobRef: "two" }]));
  render(
    <HistoryPanel client={historyEvents(call).client} onClose={() => {}} />,
  );
  const more = await screen.findByRole("button", { name: "Load more history" });
  more.focus();
  fireEvent.keyDown(more, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(more, { key: "Enter", code: "Enter" });
  await screen.findByText("2 records shown");
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("region", { name: "Operation history scroll area" }),
    ),
  );
});
