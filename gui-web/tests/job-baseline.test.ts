import { afterEach, expect, test, vi } from "vitest";
import type {
  AnalysisClient,
  AnalysisJobListRow,
  EventEnvelope,
  ScanLaneStatus,
} from "../src/ipc";
import { watchJobBaseline } from "../src/state/job-baseline";

type Deferred = {
  resolve: (rows: AnalysisJobListRow[]) => void;
  reject: (error: Error) => void;
};
const row = (sequence: number, terminal = false): AnalysisJobListRow => ({
  jobRef: "job",
  kind: "scan.metadata",
  scopeSummary: "scope",
  state: terminal ? "done" : "running",
  waitReason: "unknown",
  priority: "normal",
  supportedControls: [],
  sequence,
  ...(terminal
    ? {
        terminal: {
          state: "done",
          sequence,
          completed: 0,
          overwritten: 0,
          failed: 0,
          skipped: 0,
          unprocessed: 0,
          historyRecorded: true,
        },
      }
    : {}),
});
const event = (topic: string, sequence = 0): EventEnvelope => ({
  topic,
  seq: sequence,
  contractVersion: 1,
  occurredAt: "1970-01-01T00:00:00Z",
  jobRef: "job",
  body: { sequence },
});

function laneStatus(sequence: number): ScanLaneStatus {
  return {
    sequence,
    lanes: [
      {
        laneIndex: 0,
        rootIndexes: [0],
        inFlight: 1,
        limit: 2,
        waitReason: "none",
        waitConfirmed: true,
        remainingDirectories: "5",
        failures: "0",
      },
    ],
    observedAt: "1970-01-01T00:00:00Z",
  };
}

function laneEvent(sequence: number): EventEnvelope {
  return { ...event("scan.lane_status", sequence), body: laneStatus(sequence) };
}

function fixture() {
  const requests: Deferred[] = [];
  let listener: ((event: EventEnvelope) => void) | undefined;
  const actions: string[] = [];
  const client = {
    subscribe: (next: (event: EventEnvelope) => void) => {
      actions.push("subscribe");
      listener = next;
      return () => {
        listener = undefined;
      };
    },
    call: (name: string) => {
      if (name === "job.get") {
        actions.push("get");
        return Promise.resolve({
          jobRef: "job",
          sequence: 6,
          terminal: row(6, true).terminal,
        });
      }
      actions.push("list");
      return new Promise<AnalysisJobListRow[]>((resolve, reject) =>
        requests.push({ resolve, reject }),
      );
    },
  } as unknown as AnalysisClient;
  const changed = vi.fn();
  const failed = vi.fn();
  const laneStatusChanged = vi.fn();
  const laneStatusCleared = vi.fn();
  const gate = watchJobBaseline(
    client,
    "job",
    changed,
    failed,
    undefined,
    laneStatusChanged,
    laneStatusCleared,
  );
  return {
    actions,
    changed,
    laneStatusChanged,
    laneStatusCleared,
    failed,
    gate,
    requests,
    send: (value: EventEnvelope) => listener?.(value),
  };
}

afterEach(() => vi.useRealTimers());

test("subscribes before baseline and applies only newer events through terminal", async () => {
  const f = fixture();
  expect(f.actions).toEqual(["subscribe", "list"]);
  f.send(event("job.progress", 4));
  f.send(event("job.state_changed", 2));
  f.send(event("job.finished", 6));
  f.send(event("job.progress", 7));
  f.requests[0]?.resolve([row(3)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));
  expect(f.gate.sequence()).toBe(6);
  expect(f.changed).toHaveBeenCalledTimes(1);
  f.send(event("job.state_changed", 8));
  expect(f.gate.sequence()).toBe(6);
  expect(f.changed).toHaveBeenCalledTimes(1);
  f.gate.dispose();
});

test("lane status applies only sequences newer than its baseline and last value", async () => {
  const f = fixture();
  f.requests[0]?.resolve([row(5)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));

  f.send(laneEvent(5));
  f.send(laneEvent(6));
  f.send(laneEvent(6));
  f.send(laneEvent(7));

  expect(
    f.laneStatusChanged.mock.calls.map(([value]) => value.sequence),
  ).toEqual([6, 7]);
  expect(f.gate.sequence()).toBe(7);
  f.gate.dispose();
});

test("lane status is discarded after job.finished", async () => {
  const f = fixture();
  f.requests[0]?.resolve([row(2)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));
  f.send(laneEvent(3));
  f.send(event("job.finished", 4));
  f.send(laneEvent(5));
  expect(f.laneStatusChanged).toHaveBeenCalledTimes(1);
  f.gate.dispose();
});

test("resync clears the shown lane state and ignores values at or before its new baseline", async () => {
  const f = fixture();
  f.requests[0]?.resolve([row(2)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));
  f.send(laneEvent(3));
  f.send(event("job.resync_required"));
  f.send(laneEvent(4));
  expect(f.laneStatusCleared).toHaveBeenCalledTimes(2);
  f.requests[1]?.resolve([row(5)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));
  f.send(laneEvent(5));
  expect(f.laneStatusChanged).toHaveBeenCalledTimes(1);
  f.send(laneEvent(6));
  expect(f.laneStatusChanged).toHaveBeenCalledTimes(2);
  expect(f.laneStatusChanged.mock.calls.at(-1)?.[0].sequence).toBe(6);
  f.gate.dispose();
});

test("a second resync invalidates older responses and retains the confirmed baseline on error", async () => {
  const f = fixture();
  f.requests[0]?.resolve([row(2)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));
  f.send(event("job.resync_required"));
  f.send(event("job.state_changed", 4));
  f.send(event("job.resync_required"));
  f.send(event("job.progress", 7));
  f.requests[1]?.resolve([row(99)]);
  await Promise.resolve();
  expect(f.gate.ready()).toBe(false);
  f.requests[2]?.reject(new Error("offline"));
  await vi.waitFor(() => expect(f.failed).toHaveBeenCalledTimes(1));
  expect(f.gate.sequence()).toBe(2);
  f.send(event("job.resync_required"));
  f.requests[3]?.resolve([row(5)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));
  expect(f.gate.sequence()).toBe(5);
  f.gate.dispose();
});

test("new and unknown resync reasons both recover through job.list and job.get", async () => {
  const f = fixture();
  f.requests[0]?.resolve([row(2)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));

  for (const reason of ["subscriber_delivery_failed", "future_reason"]) {
    f.send({
      ...event("job.resync_required"),
      body: {
        reason,
        discardedEvents: 3,
        observedAt: "2026-11-12T00:00:00.000Z",
      },
    });
    const listRequest = f.requests.length - 1;
    await vi.waitFor(() => expect(f.actions.at(-1)).toBe("list"));
    f.requests[listRequest]?.resolve([{ ...row(5), state: "done" }]);
    await vi.waitFor(() =>
      expect(f.actions.filter((item) => item === "get")).toHaveLength(
        reason === "subscriber_delivery_failed" ? 1 : 2,
      ),
    );
    await vi.waitFor(() => expect(f.gate.ready()).toBe(true));
  }

  expect(f.actions).toEqual([
    "subscribe",
    "list",
    "list",
    "get",
    "list",
    "get",
  ]);
  f.gate.dispose();
});

test("timeout rejects a late result and disposal cancels future updates", async () => {
  vi.useFakeTimers();
  const f = fixture();
  vi.advanceTimersByTime(10_000);
  expect(f.failed).toHaveBeenCalledTimes(1);
  f.requests[0]?.resolve([row(50)]);
  await Promise.resolve();
  expect(f.gate.ready()).toBe(false);
  await vi.advanceTimersByTimeAsync(1_000);
  expect(f.requests).toHaveLength(2);
  f.gate.dispose();
  f.requests[1]?.resolve([row(100)]);
  await Promise.resolve();
  expect(f.changed).not.toHaveBeenCalled();
});

test("retry uses bounded backoff and a new resync or disposal cancels it", async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.requests[0]?.reject(new Error("offline"));
  await vi.advanceTimersByTimeAsync(0);
  expect(f.failed).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(999);
  expect(f.requests).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(f.requests).toHaveLength(2);
  f.requests[1]?.reject(new Error("offline again"));
  await vi.advanceTimersByTimeAsync(0);
  f.send(event("job.resync_required"));
  expect(f.requests).toHaveLength(3);
  await vi.advanceTimersByTimeAsync(2_000);
  expect(f.requests).toHaveLength(3);
  f.requests[2]?.reject(new Error("offline third"));
  await vi.advanceTimersByTimeAsync(0);
  f.gate.dispose();
  await vi.advanceTimersByTimeAsync(10_000);
  expect(f.requests).toHaveLength(3);
});

test("terminal baseline remains pending until the history outcome is confirmed", async () => {
  let finish: ((value: unknown) => void) | undefined;
  const done = row(3, true).terminal;
  const client = {
    subscribe: () => () => {},
    call: (name: string) =>
      name === "job.list"
        ? Promise.resolve([{ ...row(3), state: "done" }])
        : new Promise((resolve) => {
            finish = resolve;
          }),
  } as unknown as AnalysisClient;
  const changed = vi.fn();
  const gate = watchJobBaseline(client, "job", changed, vi.fn());
  await vi.waitFor(() => expect(finish).toBeDefined());
  expect(gate.ready()).toBe(false);
  finish?.({ jobRef: "job", sequence: 3, terminal: done });
  await vi.waitFor(() => expect(gate.ready()).toBe(true));
  expect(gate.sequence()).toBe(3);
  expect(changed).toHaveBeenCalledTimes(1);
  gate.dispose();
});

test("a terminal event buffered during recovery waits for its confirmed outcome", async () => {
  let listener: ((event: EventEnvelope) => void) | undefined;
  let finishList: ((rows: AnalysisJobListRow[]) => void) | undefined;
  const status: { finish?: (value: unknown) => void } = {};
  const client = {
    subscribe: (next: (event: EventEnvelope) => void) => {
      listener = next;
      return () => {};
    },
    call: (name: string) =>
      name === "job.list"
        ? new Promise<AnalysisJobListRow[]>((resolve) => {
            finishList = resolve;
          })
        : new Promise((resolve) => {
            status.finish = resolve;
          }),
  } as unknown as AnalysisClient;
  const changed = vi.fn();
  const gate = watchJobBaseline(client, "job", changed, vi.fn());
  listener?.(event("job.finished", 6));
  finishList?.([row(3)]);
  await vi.waitFor(() => expect(status.finish).toBeDefined());
  expect(gate.ready()).toBe(false);
  expect(changed).not.toHaveBeenCalled();
  const resolveStatus = () => status.finish;
  resolveStatus()?.({
    jobRef: "job",
    sequence: 5,
    terminal: row(5, true).terminal,
  });
  delete status.finish;
  await vi.waitFor(() => expect(status.finish).toBeDefined(), {
    timeout: 2_000,
  });
  expect(gate.ready()).toBe(false);
  resolveStatus()?.({
    jobRef: "job",
    sequence: 6,
    terminal: row(6, true).terminal,
  });
  await vi.waitFor(() => expect(gate.ready()).toBe(true));
  expect(gate.sequence()).toBe(6);
  gate.dispose();
});

test("a full early buffer or malformed job event requests a fresh baseline", async () => {
  const f = fixture();
  for (let sequence = 1; sequence <= 1025; sequence += 1)
    f.send(event("job.progress", sequence));
  expect(f.requests).toHaveLength(2);
  f.requests[0]?.resolve([row(1)]);
  await Promise.resolve();
  expect(f.gate.ready()).toBe(false);
  f.send(event("job.progress", 0));
  expect(f.requests).toHaveLength(3);
  f.requests[2]?.resolve([row(1025)]);
  await vi.waitFor(() => expect(f.gate.ready()).toBe(true));
  expect(f.gate.sequence()).toBe(1025);
  f.gate.dispose();
});
