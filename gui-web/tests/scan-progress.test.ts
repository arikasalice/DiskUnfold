import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type {
  AnalysisClient,
  AnalysisJobListRow,
  AnalysisJobStatus,
  EventEnvelope,
  JobState,
} from "../src/ipc";
import { useScanResult } from "../src/state/analysis-scan";

const timestamp = "2026-11-14T00:00:00.000Z";
let frames: FrameRequestCallback[] = [];
let hidden = false;

beforeEach(() => {
  frames = [];
  hidden = false;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) =>
    frames.push(callback),
  );
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
    frames[id - 1] = () => undefined;
  });
  Object.defineProperty(document, "hidden", {
    configurable: true,
    get: () => hidden,
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  delete (document as { hidden?: boolean }).hidden;
});

function scanClient() {
  const listeners = new Set<(event: EventEnvelope) => void>();
  const calls: string[] = [];
  const job = { state: "running" as JobState, sequence: 1, listSequence: 1 };
  const client: AnalysisClient = {
    call: (async (name: string) => {
      calls.push(name);
      if (name === "job.list")
        return [
          {
            jobRef: "scan",
            kind: "scan.metadata",
            scopeSummary: "",
            state: job.state,
            waitReason: "none",
            priority: "normal",
            supportedControls: [],
            sequence: job.listSequence,
          } satisfies AnalysisJobListRow,
        ];
      if (name === "job.get")
        return {
          jobRef: "scan",
          kind: "scan.metadata",
          state: job.state,
          sequence: job.sequence,
          resultVersion: null,
          reasonKey: null,
        } satisfies AnalysisJobStatus;
      throw new Error(`Unexpected ${name}`);
    }) as AnalysisClient["call"],
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const send = (topic: string, body: unknown) =>
    act(() => {
      for (const listener of listeners)
        listener({
          topic,
          contractVersion: 1,
          seq: 1,
          jobRef: "scan",
          occurredAt: timestamp,
          body,
        });
    });
  const progress = (sequence: number, completed: number, total?: number) =>
    send("job.progress", {
      sequence,
      unit: "items",
      completed,
      ...(total === undefined ? {} : { total }),
      totalKnown: total !== undefined,
      waitReason: "none",
      observedAt: timestamp,
    });
  return { client, calls, job, send, progress };
}

async function flush() {
  await act(async () => {
    const pending = frames.splice(0);
    for (const frame of pending) frame(0);
  });
}

async function startScan() {
  const fake = scanClient();
  let renders = 0;
  const hook = renderHook(() => {
    renders += 1;
    return useScanResult(fake.client, {
      jobRef: "scan",
      existing: false,
      scopeVersion: "scope",
      scopeFingerprint: "fingerprint",
    });
  });
  await waitFor(() => expect(frames.length).toBeGreaterThan(0));
  await flush();
  await waitFor(() =>
    expect(hook.result.current.status?.state).toBe("running"),
  );
  const reads = () => fake.calls.filter((name) => name === "job.get").length;
  return { ...fake, hook, reads, renders: () => renders };
}

test("F02 NFR5.HD1.gui-web.DD1: a burst of progress draws the latest value once per frame without status reads", async () => {
  const scan = await startScan();
  const before = scan.reads();
  for (let sequence = 2; sequence <= 101; sequence += 1)
    scan.progress(sequence, sequence * 10);
  expect(frames).toHaveLength(1);
  expect(scan.hook.result.current.progress).toBeUndefined();
  const rendersBefore = scan.renders();
  await flush();
  expect(scan.renders() - rendersBefore).toBe(1);
  expect(scan.hook.result.current.progress?.completed).toBe(1010);
  expect(scan.hook.result.current.progress?.totalKnown).toBe(false);
  expect(scan.reads()).toBe(before);
});

test("F24 IF-06: older, post-terminal and malformed progress values are not drawn", async () => {
  const scan = await startScan();
  scan.progress(10, 500);
  await flush();
  scan.progress(9, 900);
  scan.progress(10, 901);
  expect(frames).toHaveLength(0);
  await flush();
  expect(scan.hook.result.current.progress?.completed).toBe(500);

  // A malformed body is not drawn; it asks for one status read instead.
  const before = scan.reads();
  scan.send("job.progress", { sequence: 11 });
  await flush();
  await waitFor(() => expect(scan.reads()).toBe(before + 1));
  expect(scan.hook.result.current.progress?.completed).toBe(500);

  scan.send("job.finished", { sequence: 12 });
  scan.progress(13, 1300);
  await flush();
  expect(scan.hook.result.current.progress?.completed).not.toBe(1300);
});

test("F24 IF-06: a resynchronization drops undrawn values and those at or below the new baseline", async () => {
  const scan = await startScan();
  scan.progress(5, 50);
  scan.job.listSequence = 20;
  scan.job.sequence = 20;
  scan.send("job.resync_required", {
    reason: "subscriber_queue_limit",
    discardedEvents: 3,
    observedAt: timestamp,
  });
  scan.progress(15, 150);
  await flush();
  await waitFor(() =>
    expect(scan.calls.filter((name) => name === "job.list")).toHaveLength(2),
  );
  await flush();
  expect(scan.hook.result.current.progress).toBeUndefined();
  scan.progress(21, 210);
  await flush();
  expect(scan.hook.result.current.progress?.completed).toBe(210);
});

test("F02 NFR5.HD1.gui-web.DD1: a hidden window draws nothing and shows the latest value when visible", async () => {
  const scan = await startScan();
  hidden = true;
  act(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
  scan.progress(2, 20);
  scan.progress(3, 30);
  expect(frames).toHaveLength(0);
  expect(scan.hook.result.current.progress).toBeUndefined();
  hidden = false;
  act(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
  expect(scan.hook.result.current.progress?.completed).toBe(30);
});

test("F02 NFR5.HD1.gui-web.DD1: progress is not shown once the scan has ended", async () => {
  const scan = await startScan();
  scan.progress(2, 20, 40);
  await flush();
  expect(scan.hook.result.current.progress).toMatchObject({
    completed: 20,
    total: 40,
    totalKnown: true,
  });
  scan.job.state = "done";
  scan.job.sequence = 3;
  scan.send("job.state_changed", { sequence: 3 });
  await flush();
  await waitFor(() =>
    expect(scan.hook.result.current.status?.state).toBe("done"),
  );
  expect(scan.hook.result.current.progress).toBeUndefined();
});
