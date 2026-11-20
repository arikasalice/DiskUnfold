import { afterEach, expect, test, vi } from "vitest";
import type {
  AnalysisClient,
  AnalysisJobListRow,
  AnalysisJobStatus,
  EventEnvelope,
} from "../src/ipc";
import { createJobCenterStore } from "../src/state/job-center";

const time = "2026-11-21T12:00:00.000Z";
const finished = (sequence: number, state: "done" | "failed" = "done") => ({
  state,
  sequence,
  completed: 3,
  overwritten: 0,
  failed: 0,
  skipped: 0,
  unprocessed: 0,
  historyRecorded: true,
});

function row(
  jobRef: string,
  state: AnalysisJobListRow["state"] = "running",
  sequence = 1,
): AnalysisJobListRow {
  return {
    jobRef,
    kind: "scan.metadata",
    scopeSummary: "metadata scan · 1 root",
    scopeFirstRoot: "C:\\Data",
    scopeRootCount: 1,
    state,
    waitReason: "none",
    priority: "normal",
    supportedControls: ["pause", "resume", "cancel"],
    sequence,
  };
}

function rawRow(
  overrides: Record<string, unknown> = {},
  omitted: string[] = [],
): Record<string, unknown> {
  const value: Record<string, unknown> = { ...row("job-a"), ...overrides };
  for (const field of omitted) delete value[field];
  return value;
}

const invalidScopeRows: Array<[string, Record<string, unknown>]> = [
  ["only scopeFirstRoot", rawRow({}, ["scopeRootCount"])],
  ["only scopeRootCount", rawRow({}, ["scopeFirstRoot"])],
  ["zero scopeRootCount", rawRow({ scopeRootCount: 0 })],
  ["fractional scopeRootCount", rawRow({ scopeRootCount: 1.5 })],
  [
    "unsafe scopeRootCount",
    rawRow({ scopeRootCount: Number.MAX_SAFE_INTEGER + 1 }),
  ],
  ["null scopeFirstRoot", rawRow({ scopeFirstRoot: null })],
  ["null scopeRootCount", rawRow({ scopeRootCount: null })],
];

function event(
  topic: string,
  jobRef: string,
  body: unknown,
  seq = 1,
): EventEnvelope {
  return {
    topic,
    contractVersion: 1,
    seq,
    occurredAt: time,
    jobRef,
    body,
  };
}

function progress(sequence: number, completed: number) {
  return {
    sequence,
    unit: "items",
    completed,
    totalKnown: false,
    waitReason: "none",
    observedAt: time,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function clientFor(
  call: (name: string, body: Record<string, unknown>) => Promise<unknown>,
) {
  let listener: ((value: EventEnvelope) => void) | undefined;
  const order: string[] = [];
  const client = {
    subscribe(next: (value: EventEnvelope) => void) {
      order.push("subscribe");
      listener = next;
      return () => {
        listener = undefined;
      };
    },
    call(name: string, body: Record<string, unknown>) {
      order.push(name);
      return call(name, body);
    },
  } as unknown as AnalysisClient;
  return {
    client,
    order,
    send(value: EventEnvelope) {
      listener?.(value);
    },
  };
}

const stores: ReturnType<typeof createJobCenterStore>[] = [];
afterEach(() => {
  for (const store of stores.splice(0)) store.dispose();
  vi.useRealTimers();
});

test("subscribes before job.list and applies events held during the baseline", async () => {
  const list = deferred<AnalysisJobListRow[]>();
  const fixture = clientFor(() => list.promise);
  const store = createJobCenterStore(fixture.client);
  stores.push(store);

  store.start();
  expect(fixture.order).toEqual(["subscribe", "job.list"]);
  fixture.send(
    event("job.state_changed", "job-a", { sequence: 2, state: "paused" }),
  );
  list.resolve([row("job-a")]);
  await vi.waitFor(() =>
    expect(store.getSnapshot().inProgress[0]?.row.state).toBe("paused"),
  );
});

test.each(invalidScopeRows)(
  "rejects a job.list row carrying %s",
  async (_description, invalidRow) => {
    const fixture = clientFor(async () => [invalidRow]);
    const store = createJobCenterStore(fixture.client);
    stores.push(store);

    store.start();
    await vi.waitFor(() => expect(store.getSnapshot().error).toBe(true));
    expect(store.getSnapshot().cards).toEqual([]);
    expect(store.getSnapshot().loading).toBe(false);
  },
);

test("accepts job.list rows with both optional scope fields omitted", async () => {
  const fixture = clientFor(async () => [
    rawRow({}, ["scopeFirstRoot", "scopeRootCount"]),
  ]);
  const store = createJobCenterStore(fixture.client);
  stores.push(store);

  store.start();
  await vi.waitFor(() => expect(store.getSnapshot().loading).toBe(false));
  expect(store.getSnapshot().cards).toHaveLength(1);
  expect(store.getSnapshot().error).toBe(false);
});

test("jobs that screens run for themselves stay out of the lists and notices", async () => {
  const fixture = clientFor(async () => [
    row("job-a"),
    { ...row("probe"), kind: "job.kind.target_probe" },
  ]);
  const onFinished = vi.fn();
  const store = createJobCenterStore(fixture.client, onFinished);
  stores.push(store);
  store.start();
  await vi.waitFor(() => expect(store.getSnapshot().loading).toBe(false));

  expect(store.getSnapshot().inProgress.map((card) => card.row.jobRef)).toEqual(
    ["job-a"],
  );
  fixture.send(event("job.finished", "probe", finished(2), 2));
  expect(store.getSnapshot().recentlyFinished).toEqual([]);
  expect(onFinished).not.toHaveBeenCalled();
});

test("ignores stale and duplicate sequences and discards events after terminal", async () => {
  const fixture = clientFor(async () => [row("job-a")]);
  const store = createJobCenterStore(fixture.client);
  stores.push(store);
  store.start();
  await vi.waitFor(() => expect(store.getSnapshot().loading).toBe(false));

  fixture.send(event("job.progress", "job-a", progress(2, 20), 2));
  fixture.send(event("job.progress", "job-a", progress(2, 99), 3));
  fixture.send(event("job.progress", "job-a", progress(1, 10), 4));
  expect(store.getSnapshot().cards[0]?.progress?.completed).toBe(20);
  expect(store.getSnapshot().cards[0]?.sequence).toBe(2);

  fixture.send(event("job.finished", "job-a", finished(3), 5));
  fixture.send(event("job.progress", "job-a", progress(4, 40), 6));
  fixture.send(
    event("job.state_changed", "job-a", { sequence: 5, state: "running" }, 7),
  );
  expect(store.getSnapshot().cards[0]?.row.state).toBe("done");
  expect(store.getSnapshot().cards[0]?.sequence).toBe(3);
  expect(store.getSnapshot().cards[0]?.progress?.completed).toBe(20);
  expect(
    store.getSnapshot().recentlyFinished.map((card) => card.row.jobRef),
  ).toEqual(["job-a"]);
});

test("resync keeps the last cards and progress until a fresh baseline arrives", async () => {
  const retry = deferred<AnalysisJobListRow[]>();
  let lists = 0;
  const fixture = clientFor(async () => {
    lists += 1;
    return lists === 1 ? [row("job-a")] : retry.promise;
  });
  const store = createJobCenterStore(fixture.client);
  stores.push(store);
  store.start();
  await vi.waitFor(() => expect(store.getSnapshot().loading).toBe(false));
  fixture.send(event("job.progress", "job-a", progress(2, 8), 2));

  fixture.send(event("job.resync_required", "job-a", {}, 3));
  expect(store.getSnapshot().resyncing).toBe(true);
  expect(store.getSnapshot().cards[0]?.progress?.completed).toBe(8);
  expect(store.getSnapshot().cards[0]?.row.jobRef).toBe("job-a");
  fixture.send(event("job.progress", "job-a", progress(5, 12), 4));
  retry.resolve([row("job-a", "running", 4)]);
  await vi.waitFor(() =>
    expect(store.getSnapshot().cards[0]?.progress?.completed).toBe(12),
  );
  expect(lists).toBe(2);
  expect(store.getSnapshot().cards[0]?.sequence).toBe(5);
});

test("orders active tasks by state and start order and caps session finishes at five", async () => {
  const orderingRows = [
    row("queued-a", "queued"),
    row("running-a"),
    row("paused-a", "paused"),
    row("running-b"),
    row("queued-b", "queued"),
  ];
  const orderingClient = clientFor(async () => orderingRows);
  const orderingStore = createJobCenterStore(orderingClient.client);
  stores.push(orderingStore);
  orderingStore.start();
  await vi.waitFor(() =>
    expect(orderingStore.getSnapshot().loading).toBe(false),
  );
  expect(
    orderingStore.getSnapshot().inProgress.map((card) => card.row.jobRef),
  ).toEqual(["running-a", "running-b", "queued-a", "queued-b", "paused-a"]);

  const finishRows = Array.from({ length: 7 }, (_, index) =>
    row(`finish-${index}`),
  );
  const onFinished = vi.fn();
  const fixture = clientFor(async () => finishRows);
  const store = createJobCenterStore(fixture.client, onFinished);
  stores.push(store);
  store.start();
  await vi.waitFor(() => expect(store.getSnapshot().loading).toBe(false));

  for (let index = 0; index < 7; index += 1) {
    const jobRef = `finish-${index}`;
    fixture.send(event("job.finished", jobRef, finished(2), index + 1));
  }
  expect(
    store.getSnapshot().recentlyFinished.map((card) => card.row.jobRef),
  ).toEqual(["finish-6", "finish-5", "finish-4", "finish-3", "finish-2"]);
  expect(onFinished).toHaveBeenCalledTimes(7);
});

test("shows neutral control outcomes and an error when sending fails", async () => {
  const terminal = finished(2);
  const fixture = clientFor(async (name, body) => {
    if (name === "job.list") return [row("job-a"), row("job-b")];
    if (name === "job.pause") {
      if (body.jobRef === "job-b") throw new Error("offline");
      return {
        outcome: "no_effect",
        state: "running",
        reasonKey: null,
        sequence: 1,
      };
    }
    if (name === "job.resume")
      return {
        outcome: "unsupported",
        state: "running",
        reasonKey: null,
        sequence: 1,
      };
    if (name === "job.cancel")
      return {
        outcome: "already_terminal",
        state: "done",
        reasonKey: null,
        sequence: 2,
      };
    if (name === "job.get") {
      if (typeof body.jobRef !== "string") throw new Error("Missing jobRef");
      return {
        jobRef: body.jobRef,
        kind: "scan.metadata",
        state: "done",
        sequence: 2,
        resultVersion: null,
        reasonKey: null,
        terminal,
      } satisfies AnalysisJobStatus;
    }
    throw new Error(`Unexpected command ${name}`);
  });
  const store = createJobCenterStore(fixture.client);
  stores.push(store);
  store.start();
  await vi.waitFor(() => expect(store.getSnapshot().loading).toBe(false));

  await store.control("job-a", "pause");
  expect(store.getSnapshot().feedback.get("job-a")?.kind).toBe("neutral");
  await store.control("job-a", "resume");
  expect(store.getSnapshot().feedback.get("job-a")?.command).toBe("resume");
  await store.control("job-a", "cancel");
  await vi.waitFor(() =>
    expect(store.getSnapshot().recentlyFinished[0]?.row.jobRef).toBe("job-a"),
  );
  await store.control("job-b", "pause");
  expect(store.getSnapshot().feedback.get("job-b")?.kind).toBe("error");
});
