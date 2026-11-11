import { afterEach, expect, test, vi } from "vitest";
import type {
  AnalysisClient,
  AnalysisJobListRow,
  EventEnvelope,
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
    call: () => {
      actions.push("list");
      return new Promise<AnalysisJobListRow[]>((resolve, reject) =>
        requests.push({ resolve, reject }),
      );
    },
  } as unknown as AnalysisClient;
  const changed = vi.fn();
  const failed = vi.fn();
  const gate = watchJobBaseline(client, "job", changed, failed);
  return {
    actions,
    changed,
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
