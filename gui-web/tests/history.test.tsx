import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
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
const row: HistoryListResponse["rows"][number] = {
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
};

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
  expect(screen.getAllByText("Unavailable").length).toBeGreaterThan(0);
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
  await screen.findByText("No history matches these filters.");
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
  await screen.findByText("No history matches these filters.");
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
  await screen.findByText("No history matches these filters.");
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
  await screen.findByText("No history matches these filters.");
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
  await screen.findByText("No history matches these filters.");
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
