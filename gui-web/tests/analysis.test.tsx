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
  startJob = () => "scan",
  readStatus = (result) => result,
  cancelJob,
  suspendJob,
  readFailures,
  readVolume,
  lineageId,
}: {
  stale?: boolean;
  earlyProbe?: EventEnvelope;
  missingJob?: boolean;
  targetList?: TargetList | Promise<TargetList>;
  readTotals?: (result: SpaceTotal) => SpaceTotal | Promise<SpaceTotal>;
  readReview?: (result: ScopeReview) => ScopeReview | Promise<ScopeReview>;
  beforeStart?: () => Promise<void>;
  startJob?: () => string;
  readStatus?: (
    result: AnalysisJobStatus,
  ) => AnalysisJobStatus | Promise<AnalysisJobStatus>;
  cancelJob?: (
    jobRef: string,
  ) => AnalysisJobControl | Promise<AnalysisJobControl>;
  suspendJob?: (
    command: "job.pause" | "job.resume",
    jobRef: string,
  ) => AnalysisJobControl | Promise<AnalysisJobControl>;
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
            jobRef: startJob(),
            existing: false,
            scopeVersion: "scope",
            scopeFingerprint: "verified",
            lineageId,
          };
        case "job.get":
          if (missingJob) throw new AnalysisError("NOT_FOUND", "job.not_found");
          return readStatus({
            jobRef: (body as { jobRef: string }).jobRef,
            kind: "scan.metadata",
            state: "partial",
            sequence: 3,
            resultVersion: "result",
            reasonKey: null,
          });
        case "job.pause":
        case "job.resume":
          if (suspendJob)
            return suspendJob(name, (body as { jobRef: string }).jobRef);
          throw new Error("Scan suspension unavailable");
        case "job.cancel":
          if (cancelJob) return cancelJob((body as { jobRef: string }).jobRef);
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
        case "query.region.open":
          return { regionRef: "browse-region" };
        case "query.region.close":
          return {};
        case "query.cancel":
          return { outcome: "no_effect" };
        case "browse.open": {
          const request = body as BrowseOpenRequest;
          const version = request.resultVersion ?? "result";
          return {
            kind: "opened",
            requestSeq: request.requestSeq,
            origin: "scan",
            location: { resultVersion: version, itemNo: 0 },
            breadcrumbs: [],
            rootKind: "scope_root",
            currentCorrespondence: false,
            page: {
              rowKind: "folder",
              queryRef: "folder-query",
              resultVersion: version,
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
            uniquePhysical: {
              state: "present",
              value: {
                size: {
                  basis: "unique_physical",
                  evidence: "estimated",
                  state: "lower_bound",
                  bytes: "1100",
                },
                measurementLimits: [
                  "space.dedup_capacity",
                  "space.identity_unsupported",
                  "space.future_limit",
                ],
              },
            },
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
  observation: Partial<TargetProbeEvent> = {},
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
    ...observation,
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

test("F01 capacity: current observations preserve zero, time, and selection", async () => {
  const checkedAt = "2026-11-10T01:02:03.000Z";
  const { client, listeners } = fixture({
    earlyProbe: probe("probe", 3, "accessible", {
      totalBytes: "2048",
      availableBytes: "0",
      observedAt: checkedAt,
    }),
  });
  render(<App analysis={client} />);
  const choice = await screen.findByRole<HTMLInputElement>("radio", {
    name: path.display,
    description: /Total: 2,048 bytes · Available: 0 bytes/,
  });
  choice.focus();
  fireEvent.click(choice);
  const description = screen.getByText(
    /Total: 2,048 bytes · Available: 0 bytes/,
  );
  const current = description.textContent;
  expect(current).toContain("Combined capacity unverified");
  expect(current).toContain("Paths may share a volume.");
  expect(current).toContain(`Checked: ${checkedAt}`);
  expect(current).not.toContain(timestamp);

  act(() => {
    for (const listener of listeners) {
      listener(
        probe("probe", 2, "accessible", {
          totalBytes: "10",
          availableBytes: "5",
          aggregationUncertain: false,
        }),
      );
      listener(
        probe("prior-job", 4, "accessible", {
          capacityReason: "os.unsupported",
          aggregationUncertain: false,
        }),
      );
    }
  });
  expect(description.textContent).toBe(current);
  expect(choice.checked).toBe(true);
  expect(document.activeElement).toBe(choice);
  expect(screen.getByText("accessible")).toBeDefined();
});

// Individual null fields are possible on the wire even though the current
// producer emits both values together or neither.
test.each([
  ["0", null, /Total: 0 bytes · Available: Could not verify/],
  [null, "0", /Total: Could not verify · Available: 0 bytes/],
] as const)(
  "F01 capacity: preserves a single reported value (%s, %s)",
  async (totalBytes, availableBytes, description) => {
    const { client } = fixture({
      earlyProbe: probe("probe", 1, "accessible", {
        totalBytes,
        availableBytes,
        aggregationUncertain: false,
      }),
    });
    render(<App analysis={client} />);
    await screen.findByRole("radio", { name: path.display, description });
    expect(
      screen.getByText(
        /No explanation is available for the unverified capacity/,
      ),
    ).toBeDefined();
    expect(screen.queryByText(/Combined capacity unverified/)).toBeNull();
  },
);

test.each([
  ["os.unsupported", "Capacity reporting is unavailable for this target."],
  ["os.query_failed", "The capacity check failed."],
  ["os.call_slow", "The capacity check did not finish in time."],
  ["scan.path_denied", "Access to this target was denied."],
  ["scan.path_vanished", "The path was no longer available."],
  ["scan.disconnected", "The target was disconnected."],
  ["path.unresolvable", "The path could not be resolved."],
  ["job.cancelled", "The capacity check was cancelled."],
  [
    "unknown.capacity",
    "No explanation is available for the unverified capacity.",
  ],
  [null, "No explanation is available for the unverified capacity."],
] as const)(
  "F01 capacity: explains unavailable observations for %s",
  async (capacityReason, explanation) => {
    const { client } = fixture({
      earlyProbe: probe("probe", 1, "accessible", {
        reasonKey: "access.internal_reason",
        capacityReason,
        aggregationUncertain: false,
      }),
    });
    render(<App analysis={client} />);
    const description = await screen.findByText(
      /Total: Could not verify · Available: Could not verify/,
    );
    expect(description.textContent).toContain(explanation);
    if (capacityReason)
      expect(description.textContent).not.toContain(capacityReason);
    expect(description.textContent).not.toContain("access.internal_reason");
    expect(screen.getByText("accessible")).toBeDefined();
  },
);

test.each(["probe", null])(
  "F01 capacity: distinguishes checking from no observation for job %s",
  async (probeJobRef) => {
    const { client, listeners } = fixture({
      targetList: { ...targets, probeJobRef },
    });
    render(<App analysis={client} />);
    await screen.findByRole("radio", { name: path.display });
    if (probeJobRef) {
      expect(screen.getByText(/Checking capacity…/)).toBeDefined();
      act(() => {
        for (const listener of listeners) {
          listener({
            topic: "job.finished",
            contractVersion: 1,
            seq: 5,
            jobRef: probeJobRef,
            occurredAt: timestamp,
            body: { state: "failed", sequence: 5 },
          });
        }
      });
    }
    const description = screen.getByText(
      /Total: Could not verify · Available: Could not verify/,
    );
    expect(description.textContent).toContain(
      "No capacity check was reported.",
    );
    expect(description.textContent).not.toContain("Checked:");
    expect(screen.queryByText(/Checking capacity…/)).toBeNull();
  },
);

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
  expect(screen.getByText(/^Type unavailable/)).toBeDefined();
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
  expect(
    allocated.getByText(
      "Incomplete measurement · Some space data could not be verified",
    ),
  ).toBeDefined();
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
  expect(
    space.getByText("Unique physical space").nextElementSibling?.textContent,
  ).toBe(
    "At least 1.0 KiB · At least 1,100 bytes · Estimated · The file identity limit was reached · File identity could not be verified · A measurement limitation was reported",
  );
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
  expect(
    within(screen.getByRole("group", { name: "Logical size" })).getByText(
      "Incomplete measurement · Reported by the file system · At least 1,023 bytes",
    ),
  ).toBeDefined();
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
  expect(cancel.getAttribute("aria-disabled")).toBe("true");
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
  expect(cancel.getAttribute("aria-disabled")).toBeNull();
});

test.each(["keep focus", "move focus"] as const)(
  "F02 F24: keyboard cancellation handles terminal focus when users %s",
  async (intent) => {
    let state: AnalysisJobStatus["state"] = "paused";
    let sequence = 3;
    const { client, calls, listeners } = fixture({
      lineageId: "cancel-focus",
      readStatus: (base) => ({
        ...base,
        state,
        sequence,
        resultVersion: null,
        scanResult: {
          lineageId: "cancel-focus",
          sequence: 1,
          resultVersion: "pinned",
        },
      }),
      cancelJob: () => ({
        outcome: "accepted",
        state: "cancel_requested",
        reasonKey: null,
        sequence: 4,
      }),
    });
    await startFixtureScan(client);
    const browse = await screen.findByRole("button", {
      name: "Browse folders",
    });
    const cancel = screen.getByRole("button", { name: "Cancel scan" });
    cancel.focus();
    fireEvent.keyDown(cancel, { key: "Enter", code: "Enter" });
    fireEvent.keyUp(cancel, { key: "Enter", code: "Enter" });
    expect(document.activeElement).toBe(cancel);
    expect(cancel.getAttribute("aria-disabled")).toBe("true");
    expect(
      screen.getByRole("progressbar", { name: "Waiting for cancellation" }),
    ).toBeDefined();
    expect(
      screen.queryByRole("progressbar", { name: "Scanning folder" }),
    ).toBeNull();
    expect(calls.filter(({ name }) => name === "job.cancel")).toHaveLength(1);
    if (intent === "move focus") browse.focus();

    state = "cancelled";
    sequence = 5;
    notifyResult(listeners, "job.resync_required", {});
    const status = await screen.findByText("Scan status: cancelled");
    expect(screen.queryByRole("button", { name: "Cancel scan" })).toBeNull();
    expect(document.activeElement).toBe(
      intent === "move focus" ? browse : status.closest('[role="status"]'),
    );
  },
);

test("F02 F24: pauses and resumes the same scan without replacing pinned totals", async () => {
  let state: AnalysisJobStatus["state"] = "running";
  let sequence = 3;
  let acknowledge: ((response: AnalysisJobControl) => void) | undefined;
  const { client, calls, listeners } = fixture({
    lineageId: "pause-lineage",
    readStatus: (base) => ({
      ...base,
      state,
      sequence,
      resultVersion: null,
      scanResult: {
        lineageId: "pause-lineage",
        sequence: 1,
        resultVersion: "pinned",
      },
    }),
    suspendJob: () =>
      new Promise((resolve) => {
        acknowledge = resolve;
      }),
  });

  await startFixtureScan(client);
  const total = await screen.findByText("At least 42 bytes");
  const pause = screen.getByRole("button", { name: "Pause scan" });
  pause.focus();
  fireEvent.click(pause);
  fireEvent.click(pause);
  expect(screen.getByText("Scan status: pause requested")).toBeDefined();
  expect(document.activeElement).toBe(pause);
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Cancel scan" })
      .disabled,
  ).toBe(false);
  expect(calls.filter(({ name }) => name === "job.pause")).toHaveLength(1);

  await act(async () => {
    acknowledge?.({
      outcome: "accepted",
      state: "pause_requested",
      reasonKey: null,
      sequence: 4,
    });
  });
  expect(screen.getByText("Scan status: pause requested")).toBeDefined();
  expect(screen.queryByText("Scan status: paused")).toBeNull();

  state = "pause_requested";
  sequence = 4;
  notifyResult(listeners, "job.resync_required", {});
  const resume = await screen.findByRole("button", { name: "Resume scan" });
  expect(resume).toBe(pause);
  expect(
    screen.getByRole("progressbar", { name: "Scanning folder" }),
  ).toBeDefined();

  state = "paused";
  sequence = 5;
  notifyResult(listeners, "job.resync_required", {});
  await screen.findByText("Scan status: paused");
  expect(
    screen.queryByRole("progressbar", { name: "Scanning folder" }),
  ).toBeNull();
  expect(screen.getByText("At least 42 bytes")).toBe(total);
  expect(screen.getByRole("button", { name: "Browse folders" })).toBeDefined();
  expect(document.activeElement).toBe(resume);

  fireEvent.click(resume);
  expect(screen.getByText("Scan status: resume requested")).toBeDefined();
  await act(async () => {
    state = "running";
    sequence = 6;
    acknowledge?.({ outcome: "accepted", state, reasonKey: null, sequence });
  });
  await screen.findByText("Scan status: running");
  expect(screen.getByRole("button", { name: "Pause scan" })).toBe(resume);
  expect(document.activeElement).toBe(resume);
  expect(screen.getByText("At least 42 bytes")).toBe(total);
  expect(calls.filter(({ name }) => name === "space.total")).toHaveLength(1);
  expect(
    calls.filter(({ name }) => name === "job.pause" || name === "job.resume"),
  ).toEqual([
    { name: "job.pause", body: { jobRef: "scan" } },
    { name: "job.resume", body: { jobRef: "scan" } },
  ]);
});

test("F02 F24: cancellation supersedes a pending pause and terminal status cannot regress", async () => {
  let state: AnalysisJobStatus["state"] = "running";
  let sequence = 3;
  let settlePause: ((response: AnalysisJobControl) => void) | undefined;
  const { client, calls, listeners } = fixture({
    readStatus: (base) => ({ ...base, state, sequence, resultVersion: null }),
    suspendJob: () =>
      new Promise((resolve) => {
        settlePause = resolve;
      }),
    cancelJob: () => ({
      outcome: "accepted",
      state: "cancel_requested",
      reasonKey: null,
      sequence: 4,
    }),
  });
  await startFixtureScan(client);
  fireEvent.click(await screen.findByRole("button", { name: "Pause scan" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel scan" }));
  await act(async () => {
    settlePause?.({
      outcome: "unsupported",
      state: "running",
      reasonKey: "job.control_unsupported",
      sequence: 3,
    });
  });
  expect(screen.getByText("Scan status: cancellation requested")).toBeDefined();
  expect(screen.queryByRole("alert")).toBeNull();
  state = "cancelled";
  sequence = 5;
  notifyResult(listeners, "job.resync_required", {});
  await screen.findByText("Scan status: cancelled");

  state = "running";
  sequence = 6;
  const reads = calls.filter(({ name }) => name === "job.get").length;
  notifyResult(listeners, "job.resync_required", {});
  await waitFor(() =>
    expect(
      calls.filter(({ name }) => name === "job.get").length,
    ).toBeGreaterThan(reads),
  );
  expect(screen.getByText("Scan status: cancelled")).toBeDefined();
  expect(screen.queryByRole("button", { name: "Pause scan" })).toBeNull();
});

test("F02 F24: a stale pause rejection cannot overwrite a newer status read", async () => {
  let sequence = 3;
  let settlePause: ((response: AnalysisJobControl) => void) | undefined;
  const { client, calls, listeners } = fixture({
    readStatus: (base) => ({
      ...base,
      state: "running",
      sequence,
      resultVersion: null,
    }),
    suspendJob: () =>
      new Promise((resolve) => {
        settlePause = resolve;
      }),
  });
  await startFixtureScan(client);
  const pause = await screen.findByRole("button", { name: "Pause scan" });
  fireEvent.click(pause);
  sequence = 4;
  const reads = calls.filter(({ name }) => name === "job.get").length;
  notifyResult(listeners, "job.resync_required", {});
  await waitFor(() =>
    expect(
      calls.filter(({ name }) => name === "job.get").length,
    ).toBeGreaterThan(reads),
  );
  await act(async () =>
    settlePause?.({
      outcome: "unsupported",
      state: "running",
      reasonKey: "job.control_unsupported",
      sequence: 3,
    }),
  );
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText("Scan status: running")).toBeDefined();
  expect(pause.getAttribute("aria-disabled")).toBeNull();
});

test("F02 F24: an old resume failure cannot affect a replacement scan", async () => {
  let firstDone = false;
  let starts = 0;
  let failResume: ((failure: Error) => void) | undefined;
  const { client, listeners } = fixture({
    startJob: () => (++starts === 1 ? "scan" : "next-scan"),
    readStatus: (base) => ({
      ...base,
      state:
        base.jobRef === "scan" ? (firstDone ? "done" : "paused") : "running",
      sequence: firstDone && base.jobRef === "scan" ? 4 : 3,
      resultVersion: null,
    }),
    suspendJob: () =>
      new Promise((_, reject) => {
        failResume = reject;
      }),
  });
  await startFixtureScan(client);
  fireEvent.click(await screen.findByRole("button", { name: "Resume scan" }));
  expect(screen.getByText("Scan status: resume requested")).toBeDefined();
  firstDone = true;
  notifyResult(listeners, "job.resync_required", {});
  await screen.findByText("Scan finished");
  fireEvent.click(screen.getByRole("button", { name: "Start scan" }));
  await screen.findByText("Scan status: running");
  await act(async () => failResume?.(new Error("Late resume failure")));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("button", { name: "Pause scan" })).toBeDefined();
});

test.each(["starting", "cancelling"] as const)(
  "F02: an older cancellation cannot change a newer scan while it is %s",
  async (phase) => {
    let firstFinished = false;
    let starts = 0;
    let resolveStart = () => {};
    const delayedStart = new Promise<void>((resolve) => {
      resolveStart = resolve;
    });
    let settleFirstCancel = () => {};
    const firstCancel = new Promise<AnalysisJobControl>((resolve, reject) => {
      settleFirstCancel = () => {
        if (phase === "starting") {
          reject(new Error("Old cancellation failed."));
        } else {
          resolve({
            outcome: "unsupported",
            state: "running",
            reasonKey: "job.control_unsupported",
            sequence: 3,
          });
        }
      };
    });
    const { client, calls, listeners } = fixture({
      beforeStart: async () => {
        starts += 1;
        if (starts === 2 && phase === "starting") await delayedStart;
      },
      startJob: () => (starts === 1 ? "scan" : "next-scan"),
      readStatus: (base) => ({
        ...base,
        state: base.jobRef === "scan" && firstFinished ? "done" : "running",
        resultVersion: null,
      }),
      cancelJob: (jobRef) =>
        jobRef === "scan"
          ? firstCancel
          : {
              outcome: "accepted",
              state: "cancel_requested",
              reasonKey: null,
              sequence: 4,
            },
    });
    await startFixtureScan(client);
    await screen.findByText("Scan status: running");
    fireEvent.click(screen.getByRole("button", { name: "Cancel scan" }));
    firstFinished = true;
    notifyResult(listeners, "job.resync_required", {});
    await screen.findByText("Scan finished");
    fireEvent.click(screen.getByRole("button", { name: "Start scan" }));
    if (phase === "starting") {
      expect(
        screen.getByRole("progressbar", { name: "Starting scan" }),
      ).toBeDefined();
    } else {
      await screen.findByText("Scan status: running");
      fireEvent.click(screen.getByRole("button", { name: "Cancel scan" }));
    }
    await act(async () => settleFirstCancel());
    expect(screen.queryByRole("alert")).toBeNull();
    if (phase === "starting") {
      resolveStart();
      await screen.findByText("Scan status: running");
    } else {
      expect(
        screen.getByText("Scan status: cancellation requested"),
      ).toBeDefined();
      expect(
        screen
          .getByRole("button", { name: "Cancel scan" })
          .getAttribute("aria-disabled"),
      ).toBe("true");
      expect(
        calls
          .filter(({ name }) => name === "job.cancel")
          .map(({ body }) => body),
      ).toEqual([{ jobRef: "scan" }, { jobRef: "next-scan" }]);
    }
  },
);

test("F02 IF-23: opens an existing partial without an event, then keeps it until explicit refresh", async () => {
  let finish = () => {};
  let finishBackground = () => {};
  let delayBackground = false;
  const delayed = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const background = new Promise<void>((resolve) => {
    finishBackground = resolve;
  });
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
    readStatus: async () => {
      if (!delayBackground) return status;
      delayBackground = false;
      await background;
      return { ...status, jobRef: "unrelated" };
    },
    readTotals: async (result) => {
      const interim = result.resultVersion === "part";
      if (!interim) await delayed;
      return {
        ...result,
        completeness: interim ? "partial" : "complete",
        sizes: result.sizes.map((value) =>
          value.size.state === "lower_bound"
            ? {
                ...value,
                size: { ...value.size, bytes: interim ? "255" : "2207" },
              }
            : value,
        ),
      };
    },
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
  const interim = screen.getByText("Result version: Interim");
  const summary = interim.closest('[role="status"]');
  expect(screen.getByText("Scan finished")).toBeDefined();
  expect(summary?.textContent).toContain(
    "You’re still viewing an interim result.",
  );
  expect(summary?.contains(latest)).toBe(false);
  expect(
    latest.compareDocumentPosition(
      screen.getByRole("button", { name: "Browse folders" }),
    ),
  ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  expect(screen.getByText("At least 255 bytes")).toBeDefined();
  expect(screen.queryByText(/2,207 bytes/)).toBeNull();
  expect(calls.filter(({ name }) => name === "space.total")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Browse folders" }));
  await screen.findByText(/^Source: scan/);
  expect(calls.find(({ name }) => name === "browse.open")?.body).toMatchObject({
    resultVersion: "part",
  });
  fireEvent.click(screen.getByRole("button", { name: "Back to analysis" }));
  // Queue the user's selection behind a status read that cannot consume it.
  delayBackground = true;
  notifyResult(listeners, "job.resync_required", {});
  await waitFor(() => expect(delayBackground).toBe(false));
  latest.focus();
  fireEvent.keyDown(latest, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(latest, { key: "Enter", code: "Enter" });
  const reading = screen.getByRole("button", {
    name: "Reading latest result…",
  });
  expect(reading).toBe(latest);
  expect(reading.getAttribute("aria-disabled")).toBe("true");
  expect(document.activeElement).toBe(reading);
  expect(screen.getByText("At least 255 bytes")).toBeDefined();
  finishBackground();
  await waitFor(() =>
    expect(calls.filter(({ name }) => name === "space.total")).toHaveLength(2),
  );
  expect(screen.getByRole("button", { name: "Reading latest result…" })).toBe(
    reading,
  );
  const started = calls.length;
  fireEvent.keyDown(reading, { key: "Enter", code: "Enter", repeat: true });
  fireEvent.keyUp(reading, { key: "Enter", code: "Enter" });
  fireEvent.click(reading);
  finish();
  await screen.findByText("Result version: Final");
  expect(
    calls.slice(started).filter(({ name }) => name === "job.get"),
  ).toHaveLength(0);
  expect(calls.filter(({ name }) => name === "space.total")).toHaveLength(2);
  const logical = within(screen.getByRole("group", { name: "Logical size" }));
  expect(logical.getByText(/2,207 bytes/)).toBeDefined();
  expect(screen.queryByText("At least 255 bytes")).toBeNull();
  expect(document.activeElement).toBe(summary);
  expect(summary?.textContent).toContain("Now showing the latest result.");
  expect(
    logical.getByText(/Incomplete measurement · Reported by the file system/),
  ).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Browse folders" }));
  await screen.findByText(/^Source: scan/);
  expect(
    calls.filter(({ name }) => name === "browse.open").at(-1)?.body,
  ).toMatchObject({
    resultVersion: "final",
  });
});

test("F02: a failed switch retains the interim totals and can be retried after focus moves", async () => {
  let published = false;
  let attempts = 0;
  let finish = () => {};
  let fail = () => {};
  const delayed = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const delayedFailure = new Promise<void>((resolve) => {
    fail = resolve;
  });
  const { client, listeners } = fixture({
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
    readTotals: async (result) => {
      if (result.resultVersion === "part")
        return { ...result, completeness: "partial" };
      if (++attempts === 1) {
        await delayedFailure;
        throw new Error("Could not read newer totals.");
      }
      await delayed;
      return { ...result, completeness: "missing" };
    },
  });
  await startFixtureScan(client);
  await screen.findByText("Result version: Interim");
  published = true;
  notifyResult(listeners, "results.version_changed", {
    lineageId: "lineage",
    sequence: 2,
  });
  const latest = await screen.findByRole("button", {
    name: "Show latest result",
  });
  latest.focus();
  fireEvent.click(latest);
  expect(screen.getByRole("button", { name: "Reading latest result…" })).toBe(
    latest,
  );
  fail();
  await within(await screen.findByRole("alert")).findByText(
    "Could not read newer totals.",
  );
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(screen.getByText("At least 42 bytes")).toBeDefined();
  expect(
    await screen.findByRole("button", { name: "Show latest result" }),
  ).toBe(latest);
  expect(document.activeElement).toBe(latest);
  fireEvent.click(latest);
  await waitFor(() => expect(attempts).toBe(2));
  expect(screen.getByRole("button", { name: "Reading latest result…" })).toBe(
    latest,
  );
  const browse = screen.getByRole("button", { name: "Browse folders" });
  browse.focus();
  finish();
  await screen.findByText("Result version: Final with scan gaps");
  expect(document.activeElement).toBe(browse);
  expect(screen.queryByText("Could not read newer totals.")).toBeNull();
});

test("F02: an expired newer result stops showing progress after resynchronizing", async () => {
  let published = false;
  let expired = true;
  const { client, calls, listeners } = fixture({
    lineageId: "lineage",
    readStatus: (base) => ({
      ...base,
      state: "running",
      resultVersion: null,
      scanResult: {
        lineageId: "lineage",
        sequence: published ? 2 : 1,
        resultVersion: published ? "newer" : "part",
      },
    }),
    readTotals: (result) => {
      if (result.resultVersion === "newer" && expired)
        throw new AnalysisError("STALE_VERSION", "result.requery_required");
      return { ...result, completeness: "partial" };
    },
  });
  await startFixtureScan(client);
  await screen.findByText("Result version: Interim");
  published = true;
  notifyResult(listeners, "results.version_changed", {
    lineageId: "lineage",
    sequence: 2,
  });
  const latest = await screen.findByRole("button", {
    name: "Show latest result",
  });
  const before = calls.length;
  fireEvent.click(latest);
  expect(screen.getByRole("button", { name: "Reading latest result…" })).toBe(
    latest,
  );
  expect(
    await screen.findByRole("button", { name: "Show latest result" }),
  ).toBe(latest);
  expect(calls.slice(before, before + 3).map(({ name }) => name)).toEqual([
    "job.get",
    "space.total",
    "job.get",
  ]);
  expect(screen.getByText("At least 42 bytes")).toBeDefined();
  expect(screen.queryByRole("alert")).toBeNull();
  expired = false;
  fireEvent.click(latest);
  await screen.findByText("Now showing the latest result.");
});

test.each(["STALE_VERSION", "NOT_FOUND", "status read failure"] as const)(
  "F03: a publication arriving after %s still needs an explicit selection",
  async (failure) => {
    let version = "interim-a";
    let sequence = 1;
    let failStatus = false;
    const { client, calls, listeners } = fixture({
      lineageId: "lineage",
      readStatus: (base) => {
        if (failStatus) {
          failStatus = false;
          throw new Error("Could not read scan status.");
        }
        return {
          ...base,
          state: version === "final-c" ? "done" : "running",
          resultVersion: version === "final-c" ? version : null,
          scanResult: {
            lineageId: "lineage",
            sequence,
            resultVersion: version,
          },
        };
      },
      readTotals: (result) => {
        if (
          result.resultVersion === "expired-b" &&
          failure !== "status read failure"
        )
          throw new AnalysisError(failure, "result.requery_required");
        return {
          ...result,
          completeness:
            result.resultVersion === "final-c" ? "complete" : "partial",
        };
      },
    });
    const publish = (next: string) => {
      version = next;
      notifyResult(listeners, "results.version_changed", {
        lineageId: "lineage",
        sequence: ++sequence,
      });
    };
    const totalsRead = () =>
      calls
        .filter(({ name }) => name === "space.total")
        .map(({ body }) => (body as { resultVersion: string }).resultVersion);

    await startFixtureScan(client);
    await screen.findByText("Result version: Interim");
    publish("expired-b");
    const latest = await screen.findByRole("button", {
      name: "Show latest result",
    });
    failStatus = failure === "status read failure";
    fireEvent.click(latest);
    expect(screen.getByRole("button", { name: "Reading latest result…" })).toBe(
      latest,
    );
    await screen.findByRole("button", { name: "Show latest result" });
    const expected =
      failure === "status read failure"
        ? ["interim-a"]
        : ["interim-a", "expired-b"];
    expect(totalsRead()).toEqual(expected);

    publish("final-c");
    await screen.findByText("Scan finished");
    await act(async () => {});
    expect(totalsRead()).toEqual(expected);
    const interim = screen.getByText("Result version: Interim");
    const summary = interim.closest('[role="status"]');
    latest.focus();
    fireEvent.keyDown(latest, { key: "Enter", code: "Enter" });
    fireEvent.keyUp(latest, { key: "Enter", code: "Enter" });
    await screen.findByText("Result version: Final");
    expect(totalsRead()).toEqual([...expected, "final-c"]);
    expect(document.activeElement).toBe(summary);
  },
);

test("F03: an in-progress switch can read a replacement during resynchronization", async () => {
  let version = "interim-a";
  const { client, calls, listeners } = fixture({
    lineageId: "lineage",
    readStatus: (base) => ({
      ...base,
      state: "running",
      resultVersion: null,
      scanResult: {
        lineageId: "lineage",
        sequence: version === "interim-a" ? 1 : version === "expired-b" ? 2 : 3,
        resultVersion: version,
      },
    }),
    readTotals: (result) => {
      if (result.resultVersion === "expired-b") {
        version = "replacement-c";
        throw new AnalysisError("STALE_VERSION", "result.requery_required");
      }
      return { ...result, completeness: "partial" };
    },
  });
  await startFixtureScan(client);
  await screen.findByText("Result version: Interim");
  version = "expired-b";
  notifyResult(listeners, "results.version_changed", {
    lineageId: "lineage",
    sequence: 2,
  });
  fireEvent.click(
    await screen.findByRole("button", { name: "Show latest result" }),
  );
  await screen.findByText("Now showing the latest result.");
  expect(
    calls.filter(({ name }) => name === "space.total").map(({ body }) => body),
  ).toMatchObject([
    { resultVersion: "interim-a" },
    { resultVersion: "expired-b" },
    { resultVersion: "replacement-c" },
  ]);
});

test.each(["partial", "cancelled"] as const)(
  "F02: a %s scan keeps the interim result separate from the job outcome",
  async (outcome) => {
    let published = false;
    const { client, listeners } = fixture({
      lineageId: "lineage",
      readStatus: (base) => ({
        ...base,
        state: published ? outcome : "running",
        resultVersion: published ? "final" : null,
        scanResult: {
          lineageId: "lineage",
          sequence: published ? 2 : 1,
          resultVersion: published ? "final" : "part",
        },
      }),
      readTotals: (result) => ({
        ...result,
        completeness:
          result.resultVersion === "part"
            ? "partial"
            : outcome === "cancelled"
              ? "cancelled"
              : "missing",
      }),
    });
    await startFixtureScan(client);
    await screen.findByText("Result version: Interim");
    published = true;
    notifyResult(listeners, "results.version_changed", {
      lineageId: "lineage",
      sequence: 2,
    });
    await screen.findByText(`Scan status: ${outcome}`);
    expect(screen.queryByText(/^Scan finished/)).toBeNull();
    expect(screen.getByText("Result version: Interim")).toBeDefined();
    fireEvent.click(
      await screen.findByRole("button", { name: "Show latest result" }),
    );
    await screen.findByText(
      outcome === "cancelled"
        ? "Result version: Cancelled"
        : "Result version: Final with scan gaps",
    );
  },
);

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

test("F01: cancelling selection discards late review and drop while cancelling only its probe", async () => {
  let resolveReview: ((value: ScopeReview) => void) | undefined;
  const { client, calls, listeners } = fixture({
    readReview: (value) =>
      new Promise((resolve) => {
        resolveReview = () => resolve(value);
      }),
  });
  const nativeDrop = createAnalysisClient().subscribeFolderDrop;
  if (!nativeDrop) throw new Error("Expected native drop subscription");
  render(<App analysis={{ ...client, subscribeFolderDrop: nativeDrop }} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await waitFor(() => expect(resolveReview).toBeDefined());
  folderDrop({ seq: 1, pending: true });
  const cancel = screen.getByRole("button", { name: "Cancel selection" });
  cancel.focus();
  fireEvent.click(cancel);
  expect(document.activeElement).toBe(cancel);
  folderDrop({ seq: 1, path: "D:\\late" });
  await act(async () => {
    resolveReview?.({} as ScopeReview);
  });
  act(() => {
    for (const listener of listeners) listener(probe("probe", 2, "accessible"));
  });
  expect(screen.getByRole("radio", { name: path.display })).toBeTruthy();
  expect(
    (
      screen.getByRole("textbox", {
        name: "Windows folder path",
      }) as HTMLInputElement
    ).value,
  ).toBe("");
  expect(
    (screen.getByRole("button", { name: "Start scan" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(calls.filter(({ name }) => name === "scan.start")).toHaveLength(0);
  expect(
    calls.filter(({ name }) => name === "job.cancel").map(({ body }) => body),
  ).toEqual([{ jobRef: "probe" }]);
});

test("F01: cancelling before target list returns cancels its late probe and ignores its list", async () => {
  let resolveTargets: ((value: TargetList) => void) | undefined;
  const pending = new Promise<TargetList>((resolve) => {
    resolveTargets = resolve;
  });
  const { client, calls } = fixture({ targetList: pending });
  render(<App analysis={client} />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Cancel selection" }),
  );
  await act(async () => {
    resolveTargets?.(targets);
  });
  await waitFor(() =>
    expect(
      calls.filter(({ name }) => name === "job.cancel").map(({ body }) => body),
    ).toEqual([{ jobRef: "probe" }]),
  );
  expect(screen.queryByRole("radio", { name: path.display })).toBeNull();
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
});

test("F01: cancelling a new selection preserves the prior scan and its result", async () => {
  const { client, calls } = fixture();
  await startFixtureScan(client);
  const total = await screen.findByText("At least 42 bytes");
  fireEvent.change(
    screen.getByRole("textbox", { name: "Windows folder path" }),
    { target: { value: "D:\\new" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Cancel selection" }));
  expect(screen.getByText("At least 42 bytes")).toBe(total);
  expect(calls.filter(({ name }) => name === "scan.start")).toHaveLength(1);
  expect(
    calls.filter(({ name }) => name === "job.cancel").map(({ body }) => body),
  ).toEqual([{ jobRef: "probe" }]);
});

test("F01: a probe finished before its delayed list is not cancelled", async () => {
  let resolveTargets: ((value: TargetList) => void) | undefined;
  const { client, calls, listeners } = fixture({
    targetList: new Promise<TargetList>((resolve) => {
      resolveTargets = resolve;
    }),
  });
  render(<App analysis={client} />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Cancel selection" }),
  );
  act(() => {
    for (const listener of listeners)
      listener({
        topic: "job.finished",
        contractVersion: 1,
        seq: 2,
        jobRef: "probe",
        occurredAt: timestamp,
        body: { sequence: 2 },
      });
  });
  await act(async () => {
    resolveTargets?.(targets);
  });
  expect(calls.filter(({ name }) => name === "job.cancel")).toHaveLength(0);
  expect(calls.filter(({ name }) => name === "scan.start")).toHaveLength(0);
});

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
  expect(
    screen.getByText("Drop one accessible local folder. No path was changed."),
  ).toBeDefined();
  expect(
    (
      screen.getByRole("textbox", {
        name: "Windows folder path",
      }) as HTMLInputElement
    ).value,
  ).toBe(path.display);
});

test.each([
  ["Review scope", "scope.review", 1],
  ["Start scan", "scan.start", 0],
] as const)(
  "F01: a pending folder verification blocks %s without losing focus",
  async (label, command, previousCalls) => {
    const { client, calls } = fixture();
    const nativeDrop = createAnalysisClient().subscribeFolderDrop;
    if (!nativeDrop) throw new Error("Expected native drop subscription");
    render(<App analysis={{ ...client, subscribeFolderDrop: nativeDrop }} />);
    fireEvent.click(await screen.findByRole("radio", { name: path.display }));
    fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
    const start = screen.getByRole<HTMLButtonElement>("button", {
      name: "Start scan",
    });
    await waitFor(() => expect(start.disabled).toBe(false));
    const action = screen.getByRole("button", { name: label });
    action.focus();
    act(() => {
      folderDrop({ seq: 1, pending: true });
      fireEvent.click(action);
    });
    expect(calls.filter(({ name }) => name === command)).toHaveLength(
      previousCalls,
    );
    expect(
      screen.getByText("Checking dropped folder…").closest('[role="status"]'),
    ).not.toBeNull();
    expect(document.activeElement).toBe(action);
    expect(action.getAttribute("aria-disabled")).toBe("true");

    folderDrop({ seq: 1, reason: "invalid_target" });
    expect(screen.queryByText("Checking dropped folder…")).toBeNull();
    expect(screen.getByRole("alert").textContent).toContain(
      "No path was changed.",
    );
    expect(screen.getByLabelText("Reviewed scope").textContent).toContain(
      path.display,
    );
    expect(start.disabled).toBe(false);
    expect(action.hasAttribute("aria-disabled")).toBe(false);
    expect(document.activeElement).toBe(action);

    folderDrop({ seq: 2, pending: true });
    folderDrop({ seq: 2, path: "D:\\Retry" });
    expect(
      screen.getByRole<HTMLInputElement>("textbox", {
        name: "Windows folder path",
      }).value,
    ).toBe("D:\\Retry");
    expect(start.disabled).toBe(true);
    expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
  },
);

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
  expect(screen.getByText("Checking dropped folder…")).toBeDefined();
  fireEvent.change(input, { target: { value: "D:\\Manual" } });
  folderDrop({ seq: 1, path: "D:\\Late" });
  expect(input.value).toBe("D:\\Manual");
  expect(screen.queryByText("Checking dropped folder…")).toBeNull();

  folderDrop({ seq: 2, pending: true });
  folderDrop({ seq: 3, pending: true });
  folderDrop({ seq: 2, reason: "invalid_target" });
  expect(screen.queryByText(/^Drop one accessible local folder/)).toBeNull();
  expect(screen.getByText("Checking dropped folder…")).toBeDefined();
  folderDrop({ seq: 3, path: "D:\\Newest" });
  expect(input.value).toBe("D:\\Newest");
  expect(screen.queryByText("Checking dropped folder…")).toBeNull();
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
});

test.each(["finished", "rejected"] as const)(
  "F01: a drop after Start in the same render cannot block the next scope when the scan is %s",
  async (outcome) => {
    let scanFinished = false;
    let settleStart = () => {};
    const delayed = new Promise<void>((resolve, reject) => {
      settleStart = () => {
        if (outcome === "rejected") reject(new Error("Scan was not started."));
        else resolve();
      };
    });
    const { client, listeners } = fixture({
      beforeStart: () => delayed,
      readStatus: (base) => ({
        ...base,
        state: scanFinished ? "done" : "running",
        resultVersion: null,
      }),
    });
    const nativeDrop = createAnalysisClient().subscribeFolderDrop;
    if (!nativeDrop) throw new Error("Expected native drop subscription");
    render(<App analysis={{ ...client, subscribeFolderDrop: nativeDrop }} />);
    fireEvent.click(await screen.findByRole("radio", { name: path.display }));
    fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
    const start = screen.getByRole<HTMLButtonElement>("button", {
      name: "Start scan",
    });
    await waitFor(() => expect(start.disabled).toBe(false));

    act(() => {
      fireEvent.click(start);
      folderDrop({ seq: 1, pending: true });
    });
    expect(
      screen.getByRole("progressbar", { name: "Starting scan" }),
    ).toBeDefined();
    folderDrop({ seq: 1, path: "D:\\During start" });
    settleStart();
    if (outcome === "finished") {
      await screen.findByText("Scan status: running");
      folderDrop({ seq: 2, pending: true });
      folderDrop({ seq: 2, path: "D:\\During scan" });
      scanFinished = true;
      notifyResult(listeners, "job.resync_required", {});
      await screen.findByText("Scan finished");
    } else {
      await screen.findByText("Scan was not started.");
    }

    expect(screen.queryByText("Checking dropped folder…")).toBeNull();
    expect(start.disabled).toBe(false);
    expect(start.hasAttribute("aria-disabled")).toBe(false);
    const input = screen.getByRole<HTMLInputElement>("textbox", {
      name: "Windows folder path",
    });
    expect(input.value).toBe(path.display);
    folderDrop({ seq: 3, pending: true });
    expect(screen.getByText("Checking dropped folder…")).toBeDefined();
    folderDrop({ seq: 3, path: "D:\\Next scope" });
    expect(input.value).toBe("D:\\Next scope");
    expect(start.disabled).toBe(true);
  },
);

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
  expect((await screen.findByRole("alert")).textContent).toContain(
    "One or more selected paths could not be verified.",
  );
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
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Access to a selected folder was denied.",
  );
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
