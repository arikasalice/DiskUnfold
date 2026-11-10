import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import axe from "axe-core";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "../src/app/App";
import { AnalysisError, createAnalysisClient, windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  AnalysisJobControl,
  AnalysisJobStatus,
  BrowseDetail,
  BrowseOpenRequest,
  BrowseOpenResponse,
  EventEnvelope,
  ScanFailures,
  ScanFailuresRequest,
  ScopeReview,
  SpaceDetail,
  SpaceTotal,
  TargetList,
  TargetProbeEvent,
  VolumeGapRequest,
  VolumeGapResponse,
} from "../src/ipc";

afterEach(cleanup);

const path = windowsPath("C:\\fixture");
const timestamp = "2026-11-10T00:00:00.000Z";
const targets: TargetList = {
  targets: [
    {
      kind: "internal",
      path,
      connected: true,
      volumeId: "volume",
      identitySource: "confirmed",
      access: "checking",
      reasonKey: null,
    },
  ],
  probeJobRef: "probe",
  observedAt: timestamp,
  unavailableReason: null,
};

function fixture({
  stale = false,
  earlyProbe,
  missingJob = false,
  targetList = targets,
  readTotals = (result) => result,
  readReview = (result) => result,
  beforeStart,
  readStatus = (result) => result,
  cancelJob,
  readFailures,
  readVolume,
  lineageId,
}: {
  stale?: boolean;
  earlyProbe?: EventEnvelope;
  missingJob?: boolean;
  targetList?: TargetList;
  readTotals?: (result: SpaceTotal) => SpaceTotal | Promise<SpaceTotal>;
  readReview?: (result: ScopeReview) => ScopeReview | Promise<ScopeReview>;
  beforeStart?: () => Promise<void>;
  readStatus?: (result: AnalysisJobStatus) => AnalysisJobStatus;
  cancelJob?: () => AnalysisJobControl | Promise<AnalysisJobControl>;
  readFailures?: (
    request: ScanFailuresRequest,
  ) => ScanFailures | Promise<ScanFailures>;
  readVolume?: (
    request: VolumeGapRequest,
  ) => VolumeGapResponse | Promise<VolumeGapResponse>;
  lineageId?: string;
} = {}) {
  const listeners = new Set<(event: EventEnvelope) => void>();
  const calls: { name: string; body: unknown }[] = [];
  const client: AnalysisClient = {
    call: (async (name: string, body: unknown) => {
      calls.push({ name, body });
      switch (name) {
        case "target.list":
          if (earlyProbe)
            for (const listener of listeners) listener(earlyProbe);
          return targetList;
        case "scope.review": {
          const request = body as {
            draft: ScopeReview["scope"];
            requestSeq: number;
          };
          return readReview({
            scope: request.draft,
            notices: [],
            targets: request.draft.includes.map((path) => ({
              path,
              access: "accessible",
              reasonKey: null,
            })),
            reviewFingerprint: "verified",
            observedAt: timestamp,
            requestSeq: request.requestSeq,
          });
        }
        case "scan.start":
          if (beforeStart) await beforeStart();
          if (stale) throw new AnalysisError("STALE_VERSION", "scope.changed");
          return {
            jobRef: "scan",
            existing: false,
            scopeVersion: "scope",
            scopeFingerprint: "verified",
            lineageId,
          };
        case "job.get":
          if (missingJob) throw new AnalysisError("NOT_FOUND", "job.not_found");
          return readStatus({
            jobRef: "scan",
            kind: "scan.metadata",
            state: "partial",
            sequence: 3,
            resultVersion: "result",
            reasonKey: null,
          });
        case "job.cancel":
          if (cancelJob) return cancelJob();
          throw new Error("Cancellation unavailable");
        case "space.total": {
          const request = body as { requestSeq: number; resultVersion: string };
          const result: SpaceTotal = {
            resultVersion: request.resultVersion,
            requestSeq: request.requestSeq,
            completeness: "missing",
            sizes: [
              {
                size: {
                  basis: "logical",
                  evidence: "os_reported",
                  state: "lower_bound",
                  bytes: "42",
                },
                completeness: "missing",
                measurementLimits: [],
              },
              {
                size: {
                  basis: "allocated",
                  evidence: "unknown",
                  state: "unknown",
                },
                completeness: "missing",
                measurementLimits: ["space.data_insufficient"],
              },
            ],
            missingItems: "1",
            inaccessibleRanges: null,
            untraversedLinks: "2",
          };
          return readTotals(result);
        }
        case "scan.failures":
          if (readFailures) return readFailures(body as ScanFailuresRequest);
          throw new Error("Scan issues unavailable");
        case "space.volume_gap":
          if (readVolume) return readVolume(body as VolumeGapRequest);
          throw new Error("Volume explanation unavailable");
        case "browse.open": {
          const request = body as BrowseOpenRequest;
          return {
            kind: "opened",
            requestSeq: request.requestSeq,
            origin: "scan",
            location: { resultVersion: "result", itemNo: 0 },
            breadcrumbs: [],
            rootKind: "scope_root",
            currentCorrespondence: false,
            page: {
              rowKind: "folder",
              queryRef: "folder-query",
              resultVersion: "result",
              rows: [],
              total: { state: "exact", count: 0 },
              unknownCount: 0,
              observedAt: timestamp,
              completeness: "missing",
              nextCursor: null,
            },
            sortStrategies: [],
            predicateStrategies: [],
          } satisfies BrowseOpenResponse;
        }
        case "browse.detail": {
          const unavailable = {
            state: "unknown",
            reasonKey: "ref.unavailable",
          } as const;
          const result: BrowseDetail = {
            item: { resultVersion: "result", itemNo: 0 },
            displayPath: { state: "present", value: path.display },
            displayName: { state: "present", value: "fixture" },
            kind: { state: "present", value: "directory" },
            logicalSize: {
              state: "present",
              value: {
                basis: "logical",
                evidence: "os_reported",
                state: "exact",
                bytes: "0",
              },
            },
            allocatedSize: unavailable,
            createdAt: { state: "absent" },
            modifiedAt: unavailable,
            observedAt: { state: "present", value: timestamp },
            origin: "scan",
            completeness: "missing",
            currentCorrespondence: false,
            observationOnly: true,
          };
          return result;
        }
        case "space.detail": {
          const request = body as { item: SpaceDetail["item"] };
          const unknown = {
            state: "unknown",
            reasonKey: "space.data_insufficient",
          } as const;
          return {
            item: request.item,
            logical: {
              state: "present",
              value: {
                size: {
                  basis: "logical",
                  evidence: "os_reported",
                  state: "lower_bound",
                  bytes: "42",
                },
                measurementLimits: [],
              },
            },
            allocated: unknown,
            uniquePhysical: unknown,
            exclusive: unknown,
            shared: unknown,
            linkGroup: { state: "absent" },
            auxiliary: {
              state: "unknown",
              reasonKey: "space.aux_not_included",
            },
            exclusiveDecision: {
              confirmed: false,
              reasonKey: "space.sharing_not_collected",
            },
            observedAt: unknown,
            capabilities: unknown,
          } satisfies SpaceDetail;
        }
        default:
          throw new Error(`Unexpected command ${name}`);
      }
    }) as AnalysisClient["call"],
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  return { client, calls, listeners };
}

function probe(
  jobRef: string,
  sequence: number,
  access: TargetProbeEvent["access"],
): EventEnvelope {
  const body: TargetProbeEvent = {
    sequence,
    access,
    target: path,
    reasonKey: null,
    totalBytes: null,
    availableBytes: null,
    capacityReason: null,
    aggregationUncertain: true,
    observedAt: timestamp,
  };
  return {
    topic: "target.status_changed",
    contractVersion: 1,
    seq: sequence,
    jobRef,
    occurredAt: timestamp,
    body,
  };
}

test("F01: an early probe is applied and older or unrelated probes are ignored", async () => {
  const { client, listeners } = fixture({
    earlyProbe: probe("probe", 3, "accessible"),
  });
  render(<App analysis={client} />);
  await screen.findByText("accessible");
  act(() => {
    for (const listener of listeners) {
      listener(probe("prior-job", 4, "missing"));
      listener(probe("probe", 2, "denied"));
    }
  });
  expect(screen.getByText("accessible")).toBeDefined();
  expect(screen.queryByText("missing")).toBeNull();
  expect(screen.queryByText("denied")).toBeNull();
});

test("F01: a target with an unavailable type can be selected and reviewed", async () => {
  const { client, calls } = fixture({
    targetList: {
      ...targets,
      targets: targets.targets.map((target) => ({ ...target, kind: null })),
    },
    earlyProbe: probe("probe", 1, "accessible"),
  });
  render(<App analysis={client} />);
  const choice = await screen.findByRole("radio", { name: path.display });
  expect(screen.getByText("Type unavailable")).toBeDefined();
  expect(screen.getByText("accessible")).toBeDefined();
  fireEvent.click(choice);
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await waitFor(() =>
    expect(
      calls.find(({ name }) => name === "scope.review")?.body,
    ).toMatchObject({
      draft: { includes: [path] },
    }),
  );
});

test("F01: slow drive listing leaves direct folder review available", async () => {
  const { client, calls } = fixture({
    targetList: {
      targets: [],
      probeJobRef: null,
      observedAt: timestamp,
      unavailableReason: "os.call_slow",
    },
  });
  render(<App analysis={client} />);
  await screen.findByText(/Drive listing is taking longer/);
  fireEvent.change(
    screen.getByRole("textbox", { name: "Windows folder path" }),
    {
      target: { value: path.display },
    },
  );
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await waitFor(() =>
    expect(
      calls.find(({ name }) => name === "scope.review")?.body,
    ).toMatchObject({
      draft: { includes: [path] },
    }),
  );
});

test("F01: a probe that finishes before publishing an observation leaves the target unknown", async () => {
  const { client, listeners } = fixture({
    earlyProbe: {
      topic: "job.finished",
      contractVersion: 1,
      seq: 5,
      jobRef: "probe",
      occurredAt: timestamp,
      body: { state: "failed", sequence: 5 },
    },
  });
  render(<App analysis={client} />);
  await screen.findByText("unknown");
  act(() => {
    for (const listener of listeners) listener(probe("probe", 6, "accessible"));
  });
  expect(screen.getByText("unknown")).toBeDefined();
  expect(screen.queryByText("accessible")).toBeNull();
});

test("F01–F03: reviewed scope starts a scan and reads its pinned partial result", async () => {
  const { client, calls } = fixture();
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  const start = screen.getByRole("button", {
    name: "Start scan",
  }) as HTMLButtonElement;
  expect(start.disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  expect(await screen.findByText("At least 42 bytes")).toBeDefined();
  expect(screen.getByText("Unavailable")).toBeDefined();
  expect(
    screen.getByText("Result version: Final with scan gaps"),
  ).toBeDefined();
  const logical = within(screen.getByRole("group", { name: "Logical size" }));
  expect(
    logical.getByText("Incomplete measurement · Reported by the file system"),
  ).toBeDefined();
  const allocated = within(
    screen.getByRole("group", { name: "Allocated size" }),
  );
  expect(allocated.getByText("Incomplete measurement")).toBeDefined();
  const coverage = within(screen.getByLabelText("Coverage limits"));
  expect(
    coverage.getByText("Missing items").nextElementSibling?.textContent,
  ).toBe("1");
  expect(
    coverage.getByText("Untraversed links").nextElementSibling?.textContent,
  ).toBe("2");
  expect(
    coverage.getByText("Inaccessible ranges").nextElementSibling?.textContent,
  ).toBe("Not recorded");
  expect(calls.find((call) => call.name === "scan.start")?.body).toMatchObject({
    reviewFingerprint: "verified",
  });
  expect(calls.find((call) => call.name === "space.total")?.body).toMatchObject(
    { resultVersion: "result", target: { kind: "scope_root" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "View folder details" }));
  const detail = within(
    await screen.findByLabelText("Observed folder attributes"),
  );
  expect(detail.getByText("0 bytes")).toBeDefined();
  expect(detail.getByText("No value")).toBeDefined();
  expect(detail.getAllByText("Unavailable in this scan")).toHaveLength(2);
  expect(calls.find((call) => call.name === "browse.detail")?.body).toEqual({
    resultVersion: "result",
    item: { resultVersion: "result", itemNo: 0 },
  });
  const space = within(await screen.findByLabelText("Observed item space"));
  expect(
    space.getByText("At least 42 bytes · Reported by the file system"),
  ).toBeDefined();
  expect(space.getByText("Sharing evidence was not collected")).toBeDefined();
  expect(space.getByText("Auxiliary data was not collected")).toBeDefined();
  expect(
    space.getByText("Item observed at").nextElementSibling?.textContent,
  ).toBe("Unavailable in this scan");
  expect(calls.find((call) => call.name === "space.detail")?.body).toEqual(
    calls.find((call) => call.name === "browse.detail")?.body,
  );
});

test("F03: changing size display does not request another result", async () => {
  const { client, calls } = fixture({
    readTotals(result) {
      return {
        ...result,
        sizes: result.sizes.map((entry) => ({
          ...entry,
          size:
            entry.size.state === "lower_bound"
              ? { ...entry.size, bytes: "1023" }
              : entry.size,
        })),
      };
    },
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  expect(await screen.findByText("At least 1,023 bytes")).toBeDefined();
  fireEvent.change(screen.getByRole("combobox", { name: "Unit system" }), {
    target: { value: "si" },
  });
  expect(screen.getByText("At least 1.0 kB")).toBeDefined();
  fireEvent.change(screen.getByRole("combobox", { name: "Size unit" }), {
    target: { value: "0" },
  });
  expect(screen.getByText("At least 1,023 bytes")).toBeDefined();
  fireEvent.change(screen.getByRole("combobox", { name: "Size basis" }), {
    target: { value: "allocated" },
  });
  expect(screen.queryByRole("group", { name: "Logical size" })).toBeNull();
  expect(
    within(screen.getByRole("group", { name: "Allocated size" })).getByText(
      "Unavailable",
    ),
  ).toBeDefined();
  expect(calls.filter(({ name }) => name === "space.total")).toHaveLength(1);
});

test("F01: stale scope disables start and requires review again", async () => {
  const { client, calls } = fixture({ stale: true });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole("button", {
    name: "Start scan",
  }) as HTMLButtonElement;
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  await screen.findByText(
    "The target or result changed. Review the scope again.",
  );
  expect(start.disabled).toBe(true);
  expect(calls.some((call) => call.name === "space.total")).toBe(false);
});

test("F03 IF-01: a pinned result shows loading before reading totals", async () => {
  let release = () => {};
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  const { client } = fixture({
    readTotals: async (result) => {
      await pending;
      return result;
    },
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  await screen.findByRole("progressbar", { name: "Reading space totals" });
  expect(screen.queryByText("No result to display")).toBeNull();
  release();
  expect(await screen.findByText("At least 42 bytes")).toBeDefined();
  expect(
    screen.queryByRole("progressbar", { name: "Reading space totals" }),
  ).toBeNull();
});

test("F03 IF-01: a totals response for a different result version is rejected", async () => {
  const { client } = fixture({
    readTotals: (result) => ({ ...result, resultVersion: "stale" }),
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  await screen.findByText("The space totals do not match this scan's result.");
  expect(screen.queryByText("At least 42 bytes")).toBeNull();
  expect(
    screen.queryByRole("progressbar", { name: "Reading space totals" }),
  ).toBeNull();
});

test("F24: an expired job releases the scan controls without inventing a result", async () => {
  const { client, calls } = fixture({ missingJob: true });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole("button", {
    name: "Start scan",
  }) as HTMLButtonElement;
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  await screen.findByText("Scan status: unavailable");
  expect(start.disabled).toBe(false);
  expect(screen.queryByRole("button", { name: "Cancel scan" })).toBeNull();
  expect(calls.some((call) => call.name === "space.total")).toBe(false);
});

async function startFixtureScan(client: AnalysisClient) {
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
}

test("F02: starting progress hands off to the scan receipt and its cancel action", async () => {
  let release = () => {};
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  const { client } = fixture({
    beforeStart: () => pending,
    readStatus: (base) => ({
      ...base,
      state: "running",
      resultVersion: null,
    }),
  });
  await startFixtureScan(client);
  expect(
    screen.getByRole("progressbar", { name: "Starting scan" }),
  ).toBeDefined();
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Starting scan…" })
      .disabled,
  ).toBe(true);
  expect(
    screen.queryByRole("progressbar", { name: "Scanning folder" }),
  ).toBeNull();
  release();
  const progress = await screen.findByRole("progressbar", {
    name: "Scanning folder",
  });
  const cancel = screen.getByRole("button", { name: "Cancel scan" });
  expect(cancel.compareDocumentPosition(progress)).toBe(
    Node.DOCUMENT_POSITION_FOLLOWING,
  );
  expect(
    screen.queryByRole("progressbar", { name: "Starting scan" }),
  ).toBeNull();
});

test("F03: browse precedes result evidence and returning restores focus", async () => {
  const { client, calls } = fixture();
  await startFixtureScan(client);
  const evidence = await screen.findByText("At least 42 bytes");
  const browse = screen.getByRole("button", { name: "Browse folders" });
  expect(browse.compareDocumentPosition(evidence)).toBe(
    Node.DOCUMENT_POSITION_FOLLOWING,
  );
  fireEvent.click(browse);
  await waitFor(() =>
    expect(
      calls.find(({ name }) => name === "browse.open")?.body,
    ).toMatchObject({
      resultVersion: "result",
    }),
  );
  const back = screen.getByRole("button", { name: "Back to analysis" });
  expect(document.activeElement).toBe(back);
  fireEvent.click(back);
  expect(document.activeElement).toBe(browse);
  expect(screen.getByText("At least 42 bytes")).toBe(evidence);
});

function notifyResult(
  listeners: Set<(event: EventEnvelope) => void>,
  topic: string,
  body: unknown,
) {
  act(() => {
    for (const listener of listeners) {
      listener({
        topic,
        body,
        seq: 1,
        contractVersion: 1,
        occurredAt: timestamp,
      });
    }
  });
}

test("F02 NFR5: a burst of status events results in one read per frame", async () => {
  const { client, calls, listeners } = fixture();
  await startFixtureScan(client);
  await screen.findByText("Scan status: partial");
  const readCount = () => calls.filter(({ name }) => name === "job.get").length;
  const before = readCount();
  const frames: FrameRequestCallback[] = [];
  const clock = vi
    .spyOn(window, "requestAnimationFrame")
    .mockImplementation((callback) => frames.push(callback));
  try {
    for (let sequence = 1; sequence <= 100; sequence += 1) {
      await act(async () => {
        for (const listener of listeners) {
          listener({
            topic: "job.progress",
            contractVersion: 1,
            jobRef: "scan",
            seq: sequence,
            occurredAt: timestamp,
            body: { sequence },
          });
        }
      });
    }
    expect(readCount()).toBe(before);
    expect(frames).toHaveLength(1);
    await act(async () => frames[0]?.(0));
    expect(readCount()).toBe(before + 1);
  } finally {
    clock.mockRestore();
  }
});

test("F02 FR10: cancellation is announced before acknowledgement and a rejection clears it", async () => {
  let acknowledge: ((response: AnalysisJobControl) => void) | undefined;
  const { client, calls } = fixture({
    readStatus: (base) => ({
      ...base,
      state: "running",
      resultVersion: null,
    }),
    cancelJob: () =>
      new Promise((resolve) => {
        acknowledge = resolve;
      }),
  });
  await startFixtureScan(client);
  await screen.findByText("Scan status: running");
  const cancel = screen.getByRole<HTMLButtonElement>("button", {
    name: "Cancel scan",
  });
  fireEvent.click(cancel);
  expect(screen.getByText("Scan status: cancellation requested")).toBeDefined();
  expect(cancel.disabled).toBe(true);
  expect(calls.filter(({ name }) => name === "job.cancel")).toHaveLength(1);
  await act(async () => {
    acknowledge?.({
      outcome: "unsupported",
      state: "running",
      reasonKey: "job.control_unsupported",
      sequence: 3,
    });
  });
  expect(
    screen.getByText("Cancellation is unavailable for this scan."),
  ).toBeDefined();
  expect(screen.getByText("Scan status: running")).toBeDefined();
  expect(cancel.disabled).toBe(false);
});

test("F02 IF-23: opens an existing partial without an event, then keeps it until explicit refresh", async () => {
  let status: AnalysisJobStatus = {
    jobRef: "scan",
    kind: "scan.metadata",
    state: "running",
    sequence: 1,
    resultVersion: null,
    reasonKey: null,
    scanResult: { lineageId: "lineage", sequence: 1, resultVersion: "part" },
  };
  const { client, calls, listeners } = fixture({
    lineageId: "lineage",
    readStatus: () => status,
    readTotals: (result) => ({
      ...result,
      completeness: result.resultVersion === "part" ? "partial" : "complete",
    }),
  });
  await startFixtureScan(client);
  await screen.findByText("Result version: Interim");
  expect(screen.getByText("Scan status: running")).toBeDefined();

  const previousReads = calls.filter(({ name }) => name === "job.get").length;
  notifyResult(listeners, "results.version_changed", {
    lineageId: "other",
    sequence: 2,
  });
  expect(calls.filter(({ name }) => name === "job.get").length).toBe(
    previousReads,
  );
  status = {
    ...status,
    state: "done",
    resultVersion: "final",
    scanResult: { lineageId: "lineage", sequence: 2, resultVersion: "final" },
  };
  notifyResult(listeners, "results.version_changed", {
    lineageId: "lineage",
    sequence: 2,
  });
  const latest = await screen.findByRole("button", {
    name: "Show latest result",
  });
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(calls.filter(({ name }) => name === "space.total")).toHaveLength(1);
  fireEvent.click(latest);
  await screen.findByText("Result version: Final");
  const logical = within(screen.getByRole("group", { name: "Logical size" }));
  expect(logical.getByText("At least 42 bytes")).toBeDefined();
  expect(
    logical.getByText("Incomplete measurement · Reported by the file system"),
  ).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
});

test("F02 IF-23: resynchronizes when the first partial version expires before its read", async () => {
  let published = false;
  const { client, calls } = fixture({
    lineageId: "lineage",
    readStatus: (base) => ({
      ...base,
      state: published ? "done" : "running",
      resultVersion: published ? "final" : null,
      scanResult: {
        lineageId: "lineage",
        sequence: published ? 2 : 1,
        resultVersion: published ? "final" : "part",
      },
    }),
    readTotals: (result) => {
      if (result.resultVersion === "part") {
        published = true;
        throw new AnalysisError("STALE_VERSION", "result.requery_required");
      }
      return { ...result, completeness: "complete" };
    },
  });
  await startFixtureScan(client);
  await screen.findByText("Result version: Final");
  expect(
    calls.filter(({ name }) => name === "space.total").map(({ body }) => body),
  ).toMatchObject([{ resultVersion: "part" }, { resultVersion: "final" }]);
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
});

test("F02 IF-23: resync after withdrawal removes only this failed scan's partial totals", async () => {
  let withdrawn = false;
  const { client, calls, listeners } = fixture({
    lineageId: "lineage",
    readStatus: (base) => ({
      ...base,
      state: withdrawn ? "failed" : "running",
      resultVersion: null,
      scanResult: withdrawn
        ? { lineageId: "lineage", sequence: 2 }
        : { lineageId: "lineage", sequence: 1, resultVersion: "part" },
    }),
  });
  await startFixtureScan(client);
  await screen.findByText("At least 42 bytes");
  withdrawn = true;
  notifyResult(listeners, "job.resync_required", {});
  await screen.findByText("Scan status: failed");
  expect(screen.queryByText("At least 42 bytes")).toBeNull();
  expect(calls.filter(({ name }) => name === "space.total")).toHaveLength(1);
});

test("F02 IF-05: pages within the displayed result and discards a late page after changing the filter", async () => {
  let release = () => {};
  const delayed = new Promise<void>((resolve) => {
    release = resolve;
  });
  const row = (
    displayPath: string,
    kind: "unread_range" | "untraversed_link",
  ) => ({
    kind,
    displayPath,
    stage: "enumerate" as const,
    pathRef: { rootIndex: 0, relative: [] },
    reasonKey: "scan.path_denied",
  });
  const { client, calls, listeners } = fixture({
    readFailures: async ({ resultVersion, kind, cursor }) => {
      if (kind === "untraversed_link") {
        return {
          resultVersion,
          totalCount: 1,
          rows: [row("C:\\junction", kind)],
        };
      }
      if (cursor === "third") await delayed;
      return {
        resultVersion,
        totalCount: 3,
        rows: [row(cursor ?? "first", "unread_range")],
        ...(cursor !== "third" && { nextCursor: cursor ? "third" : "second" }),
      };
    },
  });
  await startFixtureScan(client);
  await screen.findByRole("button", { name: "View scan issues" });
  const statusReads = calls.filter(({ name }) => name === "job.get").length;
  await act(async () => {
    for (const listener of listeners) {
      listener({
        topic: "job.resync_required",
        body: {},
        seq: 1,
        contractVersion: 1,
        occurredAt: timestamp,
      });
    }
  });
  expect(calls.filter(({ name }) => name === "job.get")).toHaveLength(
    statusReads + 1,
  );
  expect(
    screen.getAllByRole("button", { name: "View scan issues" }),
  ).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "View scan issues" }));
  await screen.findByText("first");
  fireEvent.click(screen.getByRole("button", { name: "Load more issues" }));
  await screen.findByText("second");
  expect(screen.getByText("Showing 2 of 3 issues")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Load more issues" }));
  fireEvent.change(screen.getByRole("combobox", { name: "Issue type" }), {
    target: { value: "untraversed_link" },
  });
  await screen.findByText("C:\\junction");
  await act(async () => release());
  expect(screen.queryByText("third")).toBeNull();
  expect(screen.queryByText("first")).toBeNull();
  expect(
    calls
      .filter(({ name }) => name === "scan.failures")
      .map(({ body }) => body),
  ).toEqual([
    { resultVersion: "result" },
    { resultVersion: "result", cursor: "second" },
    { resultVersion: "result", cursor: "third" },
    { resultVersion: "result", kind: "untraversed_link" },
  ]);
});

test("ANFR5: the analysis workspace exposes labelled controls and valid ARIA roles", async () => {
  const { client } = fixture();
  const { container } = render(<App analysis={client} />);
  await screen.findByRole("radio", { name: path.display });
  expect(
    screen.getByRole("textbox", { name: "Windows folder path" }),
  ).toBeDefined();
  const result = await axe.run(container, {
    runOnly: {
      type: "rule",
      values: [
        "label",
        "button-name",
        "aria-valid-attr",
        "aria-allowed-role",
        "aria-required-attr",
      ],
    },
  });
  expect(result.violations).toEqual([]);
});

function folderDrop(
  detail: { seq: number } & (
    { pending: true } | { path: string } | { reason: "invalid_target" }
  ),
) {
  act(() => {
    window.dispatchEvent(new CustomEvent("du://folder-drop", { detail }));
  });
}

test("F01: a dropped folder replaces the selection and requires a fresh scope review", async () => {
  const { client, calls } = fixture();
  const nativeDrop = createAnalysisClient().subscribeFolderDrop;
  if (!nativeDrop) throw new Error("Expected native drop subscription");
  render(<App analysis={{ ...client, subscribeFolderDrop: nativeDrop }} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  const review = screen.getByRole("button", { name: "Review scope" });
  const start = screen.getByRole("button", {
    name: "Start scan",
  }) as HTMLButtonElement;
  fireEvent.click(review);
  await waitFor(() => expect(start.disabled).toBe(false));
  const dropped = "D:\\한글 폴더";
  folderDrop({ seq: 1, pending: true });
  folderDrop({ seq: 1, path: dropped });
  expect(
    (
      screen.getByRole("textbox", {
        name: "Windows folder path",
      }) as HTMLInputElement
    ).value,
  ).toBe(dropped);
  expect(start.disabled).toBe(true);
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
  fireEvent.click(review);
  await waitFor(() => expect(start.disabled).toBe(false));
  expect(
    calls.filter(({ name }) => name === "scope.review").at(-1)?.body,
  ).toMatchObject({
    draft: { includes: [windowsPath(dropped)] },
  });
});

test("F01: an invalid drop gives guidance without replacing the chosen folder", async () => {
  const { client } = fixture();
  const nativeDrop = createAnalysisClient().subscribeFolderDrop;
  if (!nativeDrop) throw new Error("Expected native drop subscription");
  render(<App analysis={{ ...client, subscribeFolderDrop: nativeDrop }} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  folderDrop({ seq: 1, pending: true });
  folderDrop({ seq: 1, reason: "invalid_target" });
  expect(screen.getByText("Drop one accessible local folder.")).toBeDefined();
  expect(
    (
      screen.getByRole("textbox", {
        name: "Windows folder path",
      }) as HTMLInputElement
    ).value,
  ).toBe(path.display);
});

test("F01 FR3: a delayed folder verification cannot undo a newer edit or drop", async () => {
  const { client, calls } = fixture();
  const nativeDrop = createAnalysisClient().subscribeFolderDrop;
  if (!nativeDrop) throw new Error("Expected native drop subscription");
  render(<App analysis={{ ...client, subscribeFolderDrop: nativeDrop }} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  const input = screen.getByRole<HTMLInputElement>("textbox", {
    name: "Windows folder path",
  });
  folderDrop({ seq: 1, pending: true });
  fireEvent.change(input, { target: { value: "D:\\Manual" } });
  folderDrop({ seq: 1, path: "D:\\Late" });
  expect(input.value).toBe("D:\\Manual");

  folderDrop({ seq: 2, pending: true });
  folderDrop({ seq: 3, pending: true });
  folderDrop({ seq: 2, reason: "invalid_target" });
  expect(screen.queryByText("Drop one accessible local folder.")).toBeNull();
  folderDrop({ seq: 3, path: "D:\\Newest" });
  expect(input.value).toBe("D:\\Newest");
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
});

test("F01 FR3: multiple paths are reviewed together and the exact reviewed input starts the scan", async () => {
  const { client, calls } = fixture({
    readVolume: ({ resultVersion, requestSeq }) => ({
      resultVersion,
      requestSeq,
      volumeId: "AQ",
      start: {
        state: "available",
        observedAt: timestamp,
        totalBytes: "10",
        usedBytes: "10",
        freeBytes: "0",
        evidence: "os_reported",
      },
      end: { state: "not_attempted" },
      allocated: {
        size: {
          basis: "allocated",
          evidence: "os_reported",
          state: "lower_bound",
          bytes: "2",
        },
        completeness: "missing",
      },
      uniquePhysical: {
        size: {
          basis: "unique_physical",
          evidence: "unknown",
          state: "unknown",
        },
        completeness: "missing",
      },
      factors: [
        {
          key: "gap.inaccessible",
          value: { state: "unknown", reasonKey: "space.data_insufficient" },
        },
      ],
      remainder: {
        state: "unknown",
        reasonKey: "space.volume_report_unavailable",
      },
      osCategories: {
        state: "unavailable",
        reasonKey: "space.volume_report_unavailable",
      },
    }),
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Add path" }));
  const reviewButton = screen.getByRole<HTMLButtonElement>("button", {
    name: "Review scope",
  });
  expect(reviewButton.disabled).toBe(true);
  const second = windowsPath("D:\\Another folder");
  fireEvent.change(
    screen.getByRole("textbox", { name: "Additional folder path 2" }),
    { target: { value: second.display } },
  );
  fireEvent.click(reviewButton);
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  const reviewRequest = calls.find((call) => call.name === "scope.review")
    ?.body as { draft: unknown };
  expect(reviewRequest.draft).toMatchObject({ includes: [path, second] });
  fireEvent.click(start);
  await screen.findByText("At least 42 bytes");
  expect(calls.find((call) => call.name === "scan.start")?.body).toEqual({
    draft: reviewRequest.draft,
    reviewFingerprint: "verified",
  });
  expect(
    screen.queryByRole("button", { name: "View folder details" }),
  ).toBeNull();
  expect(calls.some((call) => call.name === "browse.detail")).toBe(false);
  fireEvent.change(screen.getByRole("combobox", { name: "Scanned folder" }), {
    target: { value: "1" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  const observations = within(
    await screen.findByLabelText("Volume observations"),
  );
  expect(observations.getByText("0 bytes")).toBeDefined();
  expect(observations.getByText("Not attempted")).toBeDefined();
  expect(
    calls.find((call) => call.name === "space.volume_gap")?.body,
  ).toMatchObject({
    resultVersion: "result",
    target: { root: { resultVersion: "result", itemNo: 1 } },
  });
  fireEvent.click(
    screen.getByRole("button", {
      name: "Compare all scanned folders on this volume",
    }),
  );
  await screen.findByText("All scanned folders on this volume");
  expect(
    calls.filter((call) => call.name === "space.volume_gap").at(-1)?.body,
  ).toMatchObject({
    resultVersion: "result",
    target: { volumeId: "AQ" },
  });
});

test("F01 FR3: normalized review preserves original selections for scan admission", async () => {
  const { client, calls } = fixture({
    readReview: (review) => ({
      ...review,
      scope: { ...review.scope, includes: review.scope.includes.slice(0, 1) },
      notices: ["scope.descendant_absorbed"],
    }),
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Add path" }));
  const second = windowsPath("C:\\fixture\\child");
  fireEvent.change(
    screen.getByRole("textbox", { name: "Additional folder path 2" }),
    { target: { value: second.display } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  expect(
    screen.getByText(
      "A selected folder is already inside another selected folder.",
    ),
  ).toBeDefined();
  fireEvent.click(start);
  await screen.findByText("At least 42 bytes");
  expect(calls.find(({ name }) => name === "scan.start")?.body).toMatchObject({
    draft: { includes: [path, second] },
  });
});

test("F01 FR3: canonical Windows review paths do not replace the original request", async () => {
  const canonical = windowsPath("\\\\?\\D:\\Folder");
  const requested = windowsPath("D:\\Folder");
  const { client, calls } = fixture({
    readReview: (review) => ({
      ...review,
      scope: { ...review.scope, includes: [canonical] },
      targets: [{ path: canonical, access: "accessible", reasonKey: null }],
    }),
  });
  render(<App analysis={client} />);
  fireEvent.change(
    screen.getByRole("textbox", { name: "Windows folder path" }),
    { target: { value: requested.display } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  await screen.findByText("At least 42 bytes");
  expect(calls.find(({ name }) => name === "scan.start")?.body).toMatchObject({
    draft: { includes: [requested] },
  });
});

test("F01 FR3: editing or removing a path rejects a late review and requires another review", async () => {
  let release: ((value: ScopeReview) => void) | undefined;
  let captured: ScopeReview | undefined;
  let count = 0;
  const { client } = fixture({
    readReview: (review) => {
      count += 1;
      if (count > 1) return review;
      captured = review;
      return new Promise<ScopeReview>((resolve) => {
        release = resolve;
      });
    },
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Add path" }));
  const input = screen.getByRole("textbox", {
    name: "Additional folder path 2",
  });
  fireEvent.change(input, { target: { value: "D:\\First" } });
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  expect(
    screen.getByRole("progressbar", { name: "Reviewing scope" }),
  ).toBeDefined();
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Reviewing scope…" })
      .disabled,
  ).toBe(true);
  fireEvent.change(input, { target: { value: "D:\\Changed" } });
  expect(
    screen.queryByRole("progressbar", { name: "Reviewing scope" }),
  ).toBeNull();
  if (!release || !captured) throw new Error("Review was not started");
  await act(async () => release?.(captured as ScopeReview));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  expect(start.disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await waitFor(() => expect(start.disabled).toBe(false));
  const summary = screen.getByLabelText("Reviewed scope");
  expect(input.compareDocumentPosition(summary)).toBe(
    Node.DOCUMENT_POSITION_FOLLOWING,
  );
  expect(summary.compareDocumentPosition(start)).toBe(
    Node.DOCUMENT_POSITION_FOLLOWING,
  );
  fireEvent.click(screen.getByRole("button", { name: "Remove path 2" }));
  expect(start.disabled).toBe(true);
  expect(
    screen.queryByRole("textbox", { name: "Additional folder path 2" }),
  ).toBeNull();
});

test("F01 FR3: rejected or incomplete reviews cannot start a partial scan", async () => {
  const { client, calls } = fixture({
    readReview: (review) => ({
      ...review,
      targets: review.targets.slice(0, 1),
    }),
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Add path" }));
  fireEvent.change(
    screen.getByRole("textbox", { name: "Additional folder path 2" }),
    { target: { value: "D:\\Denied" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await screen.findByLabelText("Reviewed scope");
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Start scan" })
      .disabled,
  ).toBe(true);
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
});

test("F01 FR3: an unavailable selection prevents the whole scope from starting", async () => {
  const { client, calls } = fixture({
    readReview: (review) => ({
      ...review,
      targets: review.targets.map((target, index) =>
        index === 1
          ? { ...target, access: "denied", reasonKey: "scan.path_denied" }
          : target,
      ),
    }),
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Add path" }));
  fireEvent.change(
    screen.getByRole("textbox", { name: "Additional folder path 2" }),
    { target: { value: "D:\\Denied" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await screen.findByLabelText("Reviewed scope");
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Start scan" })
      .disabled,
  ).toBe(true);
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
});

test("F01 FR3: failed multi-path review retains input and rejects start", async () => {
  const { client, calls } = fixture({
    readReview: () => {
      throw new AnalysisError("INVALID_ARGUMENT", "scope.target_unavailable");
    },
  });
  render(<App analysis={client} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Add path" }));
  fireEvent.change(
    screen.getByRole("textbox", { name: "Additional folder path 2" }),
    { target: { value: "D:\\Denied" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const message =
    "Check that every selected path is an accessible local folder, then review the scope again.";
  const problem = await within(
    screen.getByRole("region", { name: "Target and scope" }),
  ).findByText(message);
  const review = screen.getByRole("button", { name: "Review scope" });
  expect(problem.compareDocumentPosition(review)).toBe(
    Node.DOCUMENT_POSITION_FOLLOWING,
  );
  expect(
    within(
      screen.getByRole("region", { name: "Scan and results" }),
    ).queryByText(message),
  ).toBeNull();
  expect(
    screen.getByRole<HTMLInputElement>("textbox", {
      name: "Additional folder path 2",
    }).value,
  ).toBe("D:\\Denied");
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Start scan" })
      .disabled,
  ).toBe(true);
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
});

test("F01 FR3: multi-path controls have accessible labels and preserve the drop input", async () => {
  const { client } = fixture();
  const nativeDrop = createAnalysisClient().subscribeFolderDrop;
  if (!nativeDrop) throw new Error("Expected native drop subscription");
  const { container } = render(
    <App analysis={{ ...client, subscribeFolderDrop: nativeDrop }} />,
  );
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  const add = screen.getByRole("button", { name: "Add path" });
  add.focus();
  fireEvent.keyDown(add, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(add, { key: "Enter", code: "Enter" });
  const additional = screen.getByRole("textbox", {
    name: "Additional folder path 2",
  });
  expect(document.activeElement).toBe(additional);
  fireEvent.change(additional, { target: { value: "D:\\Kept" } });
  folderDrop({ seq: 1, pending: true });
  folderDrop({ seq: 1, path: "C:\\Dropped" });
  expect(
    screen.getByRole<HTMLInputElement>("textbox", {
      name: "Windows folder path",
    }).value,
  ).toBe("C:\\Dropped");
  expect(
    screen.getByRole<HTMLInputElement>("textbox", {
      name: "Additional folder path 2",
    }).value,
  ).toBe("D:\\Kept");
  expect(
    (
      await axe.run(container, {
        runOnly: {
          type: "rule",
          values: [
            "label",
            "button-name",
            "aria-valid-attr",
            "aria-allowed-role",
            "aria-required-attr",
          ],
        },
      })
    ).violations,
  ).toEqual([]);
});
