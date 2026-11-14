import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, test } from "vitest";
import { AnalysisError } from "../src/ipc";
import {
  followPrepared,
  isOpenSample,
  isSampleScreen,
  isSampleSource,
  useSampleSession,
} from "../src/state/sample-session";
import { sampleClient, sampleEvent, sampleSession } from "./sample-support";

test("a response without a data source counts as a sample, a real one does not", () => {
  expect(isSampleSource(undefined)).toBe(true);
  expect(isSampleSource("sample")).toBe(true);
  expect(isSampleSource("real")).toBe(false);
});

test("a closed session is not open", () => {
  expect(isOpenSample(undefined)).toBe(false);
  expect(isOpenSample(sampleSession({ state: "closed" }))).toBe(false);
  expect(isOpenSample(sampleSession())).toBe(true);
});

test("the sample screen shows for an active session, or one reset after it was active", () => {
  expect(isSampleScreen(undefined, false)).toBe(false);
  expect(isSampleScreen(sampleSession({ state: "active" }), false)).toBe(true);
  // Before the first activation the entry button shows the progress instead.
  expect(isSampleScreen(sampleSession({ state: "preparing" }), false)).toBe(
    false,
  );
  expect(isSampleScreen(sampleSession({ state: "resetting" }), true)).toBe(
    true,
  );
  expect(isSampleScreen(sampleSession({ state: "unavailable" }), true)).toBe(
    false,
  );
});

test("the prepared count follows only the loading job and only newer values", () => {
  const body = (sequence: number, completed: number, unit = "items") => ({
    topic: "job.progress",
    seq: sequence,
    contractVersion: 1,
    occurredAt: "2026-02-01T00:00:00Z",
    jobRef: "job-1",
    body: { sequence, unit, completed },
  });
  const first = followPrepared(undefined, "job-1", body(2, 10));
  expect(first?.completed).toBe(10);
  expect(followPrepared(first, "job-1", body(1, 99))).toBe(first);
  expect(followPrepared(first, "job-1", body(3, 30))?.completed).toBe(30);
  expect(followPrepared(first, "job-2", body(3, 30))).toBe(first);
  expect(followPrepared(first, "job-1", body(3, 30, "bytes"))).toBe(first);
  expect(followPrepared(undefined, undefined, body(1, 5))).toBeUndefined();
});
async function opened(reply: { current: ReturnType<typeof sampleSession> }) {
  const fake = sampleClient({
    "sample.open": () => sampleSession({ state: "preparing", sequence: 1 }),
    "sample.status": () => reply.current,
  });
  const view = renderHook(() => useSampleSession(fake.client));
  await act(async () => {
    await view.result.current.open();
  });
  return { fake, view };
}

test("events at or below the stored sequence are ignored and newer ones re-read the status", async () => {
  const reply = { current: sampleSession({ state: "preparing", sequence: 1 }) };
  const { fake, view } = await opened(reply);
  expect(view.result.current.session?.state).toBe("preparing");

  act(() => {
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "sample-1",
        state: "active",
        sequence: 1,
      }),
    );
  });
  expect(fake.count("sample.status")).toBe(0);

  reply.current = sampleSession({ sequence: 2 });
  act(() => {
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "sample-1",
        state: "active",
        sequence: 2,
      }),
    );
  });
  await waitFor(() => expect(fake.count("sample.status")).toBe(1));
  await waitFor(() =>
    expect(view.result.current.session?.observations).toHaveLength(2),
  );
  expect(view.result.current.session).toMatchObject({
    state: "active",
    sequence: 2,
  });

  act(() => {
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "sample-1",
        state: "loading",
        sequence: 2,
      }),
    );
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "other",
        state: "failed",
        sequence: 9,
      }),
    );
  });
  expect(fake.count("sample.status")).toBe(1);
  expect(view.result.current.session?.state).toBe("active");
});

test("a status response older than the stored sequence is dropped", async () => {
  const reply = { current: sampleSession({ state: "preparing", sequence: 1 }) };
  const { fake, view } = await opened(reply);
  reply.current = sampleSession({ state: "preparing", sequence: 2 });
  act(() => {
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "sample-1",
        state: "active",
        sequence: 3,
      }),
    );
  });
  await waitFor(() => expect(fake.count("sample.status")).toBe(1));
  await act(async () => {});
  expect(view.result.current.session).toMatchObject({
    state: "active",
    sequence: 3,
  });
});

test("a resync request reads the status again", async () => {
  const reply = { current: sampleSession({ state: "preparing", sequence: 1 }) };
  const { fake, view } = await opened(reply);
  reply.current = sampleSession({ sequence: 4 });
  act(() => {
    fake.emit(sampleEvent("job.resync_required", undefined));
  });
  await waitFor(() => expect(view.result.current.session?.sequence).toBe(4));
  expect(fake.count("sample.status")).toBe(1);
});

test("a second command is not sent while one is in flight", async () => {
  const fake = sampleClient({ "sample.open": () => sampleSession() });
  const view = renderHook(() => useSampleSession(fake.client));
  await act(async () => {
    void view.result.current.open();
    void view.result.current.open();
  });
  expect(fake.count("sample.open")).toBe(1);
});

test("an open session is reused instead of opening another", async () => {
  const reply = { current: sampleSession() };
  const { fake, view } = await opened(reply);
  await act(async () => {
    await view.result.current.open();
  });
  expect(fake.count("sample.open")).toBe(1);
  expect(fake.count("sample.status")).toBe(1);
});

test("close empties the session on success and on an INTERNAL warning, and keeps it when busy", async () => {
  for (const [failure, ended] of [
    [undefined, true],
    [new AnalysisError("INTERNAL", "sample.release_failed"), true],
    [new AnalysisError("NOT_FOUND", "sample.session_closed"), true],
    [new AnalysisError("BUSY", "job.queue_full"), false],
  ] as const) {
    const fake = sampleClient({
      "sample.open": () => sampleSession(),
      "sample.close": () => {
        if (failure) throw failure;
        return { sessionRef: "sample-1", state: "closed", releasedVersions: 2 };
      },
    });
    const view = renderHook(() => useSampleSession(fake.client));
    await act(async () => {
      await view.result.current.open();
    });
    let result: boolean | undefined;
    await act(async () => {
      result = await view.result.current.close();
    });
    expect(result).toBe(ended);
    expect(view.result.current.session === undefined).toBe(ended);
    expect(view.result.current.problem).toBe(
      failure?.code === "BUSY"
        ? "busy"
        : failure?.code === "NOT_FOUND"
          ? "closed"
          : undefined,
    );
  }
});
