import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AnalysisPanel } from "../src/features/analysis/AnalysisPanel";
import { VolumeExplanation } from "../src/features/analysis/VolumeExplanation";
import { AnalysisError, windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  BrowseDetail,
  EventEnvelope,
  JobState,
  ScopeReview,
  SpaceTotal,
  TargetList,
  VolumeGapRequest,
  VolumeGapResponse,
} from "../src/ipc";
import { useScanResult } from "../src/state/analysis-scan";
import { RESULT_REFLECT_INTERVAL_MS } from "../src/state/result-cadence";

/*
 * F02 FR3.HD1.gui-web.DD1 (user decision 2026-11-15, design review rule 8 and
 * rule 10): the analysis summary reflects a running scan's newer results in
 * place, at most once a second, and holds still only while text in it is
 * selected. Dialogs keep the result they opened with.
 */

let reflectOffset = 0;
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  reflectOffset = 0;
  document.getSelection()?.removeAllRanges();
});

/** Moves the cadence clock one interval ahead so the next publication is read at once. */
function passReflectInterval() {
  if (reflectOffset === 0) {
    const realNow = Date.now.bind(Date);
    vi.spyOn(Date, "now").mockImplementation(() => realNow() + reflectOffset);
  }
  reflectOffset += RESULT_REFLECT_INTERVAL_MS;
}

const path = windowsPath("C:\\fixture");
const timestamp = "2026-11-15T00:00:00.000Z";
const targets: TargetList = {
  targets: [
    {
      kind: "internal",
      path,
      connected: true,
      volumeId: "volume",
      identitySource: "confirmed",
      access: "accessible",
      reasonKey: null,
    },
  ],
  probeJobRef: "probe",
  observedAt: timestamp,
  unavailableReason: null,
};

/** "v3" carries 300 bytes and 3 missing items, so each value names its result. */
function amount(version: string) {
  return Number(version.slice(1));
}

function totals(version: string, requestSeq: number, final: boolean) {
  return {
    resultVersion: version,
    requestSeq,
    completeness: final ? "complete" : "partial",
    sizes: [
      {
        size: {
          basis: "logical",
          evidence: "os_reported",
          state: "exact",
          bytes: String(amount(version) * 100),
        },
        completeness: final ? "complete" : "partial",
        measurementLimits: [],
      },
    ],
    missingItems: String(amount(version)),
    inaccessibleRanges: "0",
    untraversedLinks: "0",
  } satisfies SpaceTotal;
}

function folderDetail(version: string): BrowseDetail {
  return {
    item: { resultVersion: version, itemNo: 0 },
    displayPath: { state: "present", value: path.display },
    displayName: { state: "present", value: "fixture" },
    kind: { state: "present", value: "directory" },
    logicalSize: {
      state: "present",
      value: {
        basis: "logical",
        evidence: "os_reported",
        state: "exact",
        bytes: String(amount(version) * 100),
      },
    },
    allocatedSize: { state: "absent" },
    createdAt: { state: "absent" },
    modifiedAt: { state: "absent" },
    observedAt: { state: "present", value: timestamp },
    origin: "scan",
    completeness: "partial",
    currentCorrespondence: false,
    observationOnly: true,
  };
}

/**
 * A scan whose newest result is \`job.version\`. Reads of any other version are
 * refused as replaced, as the engine does once a newer result is published.
 */
function liveScan() {
  const listeners = new Set<(event: EventEnvelope) => void>();
  const calls: { name: string; body: { resultVersion?: string } }[] = [];
  const job = { state: "running" as JobState, version: "v1", sequence: 1 };
  const current = (version: string | undefined) => {
    if (version !== job.version)
      throw new AnalysisError("STALE_VERSION", "result.requery_required");
  };
  const client: AnalysisClient = {
    call: (async (name: string, body: { resultVersion?: string }) => {
      calls.push({ name, body });
      switch (name) {
        case "target.list":
          return targets;
        case "scope.review": {
          const request = body as unknown as {
            draft: ScopeReview["scope"];
            requestSeq: number;
          };
          return {
            scope: request.draft,
            notices: [],
            targets: request.draft.includes.map((included) => ({
              path: included,
              access: "accessible",
              reasonKey: null,
            })),
            reviewFingerprint: "verified",
            observedAt: timestamp,
            requestSeq: request.requestSeq,
          } satisfies ScopeReview;
        }
        case "scan.start":
          return {
            jobRef: "scan",
            existing: false,
            scopeVersion: "scope",
            scopeFingerprint: "verified",
            lineageId: "lineage",
          };
        case "job.list":
          return [];
        case "job.get": {
          const ended = !["running", "paused"].includes(job.state);
          return {
            jobRef: "scan",
            kind: "scan.metadata",
            state: job.state,
            sequence: job.sequence,
            resultVersion: ended ? job.version : null,
            reasonKey: null,
            scanResult: {
              lineageId: "lineage",
              sequence: job.sequence,
              resultVersion: job.version,
            },
          } satisfies AnalysisJobStatus;
        }
        case "space.total": {
          current(body.resultVersion);
          const { requestSeq } = body as unknown as { requestSeq: number };
          return totals(job.version, requestSeq, job.state === "done");
        }
        case "browse.detail":
          current(body.resultVersion);
          return folderDetail(job.version);
        case "space.detail":
          current(body.resultVersion);
          throw new Error("Space details unavailable");
        case "browse.copy_path":
          current(body.resultVersion);
          return { outcome: "copied", count: 1 };
        default:
          throw new Error("Unexpected " + name);
      }
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
          body,
          seq: 1,
          contractVersion: 1,
          occurredAt: timestamp,
        });
    });
  /** Publishes the next result, optionally with a new job state. */
  const publish = (version: string, state?: JobState) => {
    job.version = version;
    job.sequence += 1;
    if (state) job.state = state;
    send("results.version_changed", {
      lineageId: "lineage",
      sequence: job.sequence,
    });
  };
  /** Changes only the job state and asks the panel to read it again. */
  const changeState = (state: JobState) => {
    job.state = state;
    job.sequence += 1;
    send("job.resync_required", {});
  };
  const reads = (name: string) =>
    calls
      .filter((call) => call.name === name)
      .map((call) => call.body.resultVersion);
  return { client, job, publish, changeState, reads };
}

async function startPanel() {
  const scan = liveScan();
  render(
    <AnalysisPanel
      client={scan.client}
      preferences={{
        display: { system: "iec", unit: "auto" },
        basis: "logical",
        warning: undefined,
      }}
    />,
  );
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  await screen.findByText("Result version: Interim");
  return scan;
}

const logical = () =>
  within(screen.getByRole("group", { name: "Logical size" }));

function coverage() {
  return within(screen.getByLabelText("Coverage limits"));
}

function select(node: Node) {
  const range = document.createRange();
  range.selectNodeContents(node);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  act(() => {
    document.dispatchEvent(new Event("selectionchange"));
  });
}

function clearSelection() {
  document.getSelection()!.removeAllRanges();
  act(() => {
    document.dispatchEvent(new Event("selectionchange"));
  });
}

test("F02 FR3.HD1.gui-web.DD1: a running scan's newer result replaces the summary values without a selection", async () => {
  const scan = await startPanel();
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(screen.getByText("Values update as the scan runs")).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
  expect(screen.queryByText(/Newer result available/)).toBeNull();

  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(logical().getByText("200 bytes")).toBeDefined());
  expect(coverage().getByText("2")).toBeDefined();
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
  expect(scan.reads("space.total")).toEqual(["v1", "v2"]);
});

test("F02 FR3.HD1.gui-web.DD1: the summary line follows pause and the final result replaces the interim one", async () => {
  const scan = await startPanel();
  scan.changeState("paused");
  await screen.findByText("Result so far");
  expect(screen.queryByText("Values update as the scan runs")).toBeNull();
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();

  scan.changeState("running");
  await screen.findByText("Values update as the scan runs");
  passReflectInterval();
  scan.publish("v3", "done");
  await screen.findByText("Result version: Final");
  expect(screen.getByText("Scan finished")).toBeDefined();
  expect(logical().getByText("300 bytes")).toBeDefined();
  expect(screen.queryByText("Values update as the scan runs")).toBeNull();
  expect(screen.queryByText("Result so far")).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
});

test("F02 FR3.HD1.gui-web.DD1: selecting text holds the whole summary until the selection is released", async () => {
  const scan = await startPanel();
  select(logical().getByText("100 bytes"));
  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(scan.reads("space.total")).toEqual(["v1", "v2"]));
  await act(async () => {});
  // Every value of the summary still comes from the first result.
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(coverage().getByText("1")).toBeDefined();

  clearSelection();
  expect(logical().getByText("200 bytes")).toBeDefined();
  expect(coverage().getByText("2")).toBeDefined();
  // Releasing shows what was already read; nothing is read again.
  expect(scan.reads("space.total")).toEqual(["v1", "v2"]);
});

test("F02 FR3.HD1.gui-web.DD1: a selection outside the summary does not hold it", async () => {
  const scan = await startPanel();
  select(screen.getByRole("heading", { name: "Disk space results" }));
  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(logical().getByText("200 bytes")).toBeDefined());
});

test("F02 FR3.HD1.gui-web.DD1: expanded measurement details stay open and focused while values change", async () => {
  const scan = await startPanel();
  const details = screen.getByRole("button", { name: "Measurement details" });
  fireEvent.click(details);
  details.focus();
  const options = screen.getByRole("button", { name: "Display options" });
  fireEvent.click(options);
  details.focus();
  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(logical().getByText("200 bytes")).toBeDefined());
  const measurements = within(
    screen.getByLabelText("Measurement details", { selector: "dl" }),
  );
  expect(measurements.getByText("200 bytes")).toBeDefined();
  expect(screen.getByRole("button", { name: "Measurement details" })).toBe(
    details,
  );
  expect(details.getAttribute("aria-expanded")).toBe("true");
  expect(options.getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(details);
});

test("F02 FR3.HD1.gui-web.DD1: folder details keep the result they opened with and move to the newest one for a further read", async () => {
  const scan = await startPanel();
  fireEvent.click(screen.getByRole("button", { name: "View folder details" }));
  const dialog = within(await screen.findByRole("dialog"));
  await dialog.findByText("100 bytes");
  expect(dialog.getByText("Result published")).toBeDefined();

  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(scan.reads("space.total")).toEqual(["v1", "v2"]));
  await act(async () => {});
  // The open dialog stays and keeps its values; the engine kept nothing for it.
  expect(screen.getByRole("dialog")).toBeDefined();
  expect(dialog.getByText("100 bytes")).toBeDefined();
  expect(dialog.queryByText("Updated to the latest result")).toBeNull();
  expect(scan.reads("browse.detail")).toEqual(["v1"]);

  // Copying reads the path again; the replaced result moves the dialog.
  fireEvent.click(dialog.getByRole("button", { name: "Copy path" }));
  await dialog.findByText("Updated to the latest result");
  await dialog.findByText("200 bytes");
  expect(dialog.queryByText("100 bytes")).toBeNull();
  expect(scan.reads("browse.detail")).toEqual(["v1", "v2"]);

  fireEvent.click(dialog.getByRole("button", { name: "Close details" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  fireEvent.click(screen.getByRole("button", { name: "View folder details" }));
  const reopened = within(await screen.findByRole("dialog"));
  await reopened.findByText("200 bytes");
  expect(reopened.queryByText("Updated to the latest result")).toBeNull();
});

test("F02 NFR5.HD1.gui-web.DD1: newer results are read at most once per reflect interval", async () => {
  vi.useFakeTimers({
    toFake: [
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "Date",
    ],
  });
  const frames: FrameRequestCallback[] = [];
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) =>
    frames.push(callback),
  );
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
  const scan = liveScan();
  const readTimes: number[] = [];
  const call = scan.client.call;
  scan.client.call = ((name: string, body: never) => {
    if (name === "space.total") readTimes.push(Date.now());
    return call(name as never, body);
  }) as AnalysisClient["call"];
  const settle = async (ms = 0) => {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
      for (const frame of frames.splice(0)) frame(0);
      await vi.advanceTimersByTimeAsync(0);
    });
  };
  const hook = renderHook(() =>
    useScanResult(scan.client, {
      jobRef: "scan",
      existing: false,
      scopeVersion: "scope",
      scopeFingerprint: "verified",
      lineageId: "lineage",
    }),
  );
  await settle();
  await settle();
  expect(hook.result.current.total?.resultVersion).toBe("v1");
  const started = readTimes[0]!;

  // Four publications inside one interval lead to a single read of the newest.
  for (const version of ["v2", "v3", "v4", "v5"]) {
    scan.publish(version);
    await settle(200);
  }
  expect(scan.reads("space.total")).toEqual(["v1"]);
  expect(hook.result.current.total?.resultVersion).toBe("v1");
  await settle(Math.max(0, started + RESULT_REFLECT_INTERVAL_MS - Date.now()));
  await settle();
  expect(scan.reads("space.total")).toEqual(["v1", "v5"]);
  expect(hook.result.current.total?.resultVersion).toBe("v5");

  scan.publish("v6");
  await settle(500);
  expect(scan.reads("space.total")).toEqual(["v1", "v5"]);
  await settle(RESULT_REFLECT_INTERVAL_MS);
  await settle();
  expect(scan.reads("space.total")).toEqual(["v1", "v5", "v6"]);
  for (let index = 1; index < readTimes.length; index += 1)
    expect(readTimes[index]! - readTimes[index - 1]!).toBeGreaterThanOrEqual(
      RESULT_REFLECT_INTERVAL_MS,
    );
  expect(hook.result.current.hasNewResult).toBe(false);
});

function volumeReport(version: string): VolumeGapResponse {
  return {
    resultVersion: version,
    requestSeq: 0,
    volumeId: "AQ",
    start: { state: "not_attempted" },
    end: {
      state: "available",
      observedAt: null,
      totalBytes: "1000",
      usedBytes: String(amount(version) * 100),
      freeBytes: "0",
      evidence: "os_reported",
    },
    allocated: {
      size: {
        basis: "allocated",
        state: "exact",
        bytes: "0",
        evidence: "measured",
      },
      completeness: "partial",
    },
    uniquePhysical: {
      size: {
        basis: "unique_physical",
        state: "exact",
        bytes: "0",
        evidence: "measured",
      },
      completeness: "partial",
    },
    factors: [],
    remainder: { state: "unknown", reasonKey: "space.data_insufficient" },
    osCategories: { state: "available", rows: [] },
  };
}

test("F02 FR3.HD1.gui-web.DD1: the volume explanation keeps its result and rereads the newest one to compare the whole volume", async () => {
  const requests: VolumeGapRequest[] = [];
  const details: string[] = [];
  const client = {
    call: (async (name: string, body: never) => {
      if (name === "browse.detail") {
        const { resultVersion } = body as { resultVersion: string };
        details.push(resultVersion);
        return folderDetail(resultVersion);
      }
      if (name !== "space.volume_gap") throw new Error("Unexpected " + name);
      const request = body as VolumeGapRequest;
      requests.push(request);
      return {
        ...volumeReport(request.resultVersion),
        requestSeq: request.requestSeq,
      };
    }) as AnalysisClient["call"],
    subscribe: () => () => {},
  } satisfies AnalysisClient;
  const props = {
    client,
    roots: [windowsPath("C:\\one"), windowsPath("C:\\two")],
    display: { system: "iec", unit: "auto" } as const,
  };
  const view = render(<VolumeExplanation {...props} resultVersion="v1" />);
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  const summary = within(await screen.findByLabelText("Volume usage summary"));
  expect(summary.getByText("Result published")).toBeDefined();
  expect(summary.getByText(/100 bytes/)).toBeDefined();

  view.rerender(<VolumeExplanation {...props} resultVersion="v2" />);
  expect(summary.getByText(/100 bytes/)).toBeDefined();
  expect(requests.map(({ resultVersion }) => resultVersion)).toEqual(["v1"]);

  fireEvent.click(
    screen.getByRole("button", {
      name: "Compare all scanned folders on this volume",
    }),
  );
  await screen.findByText("Updated to the latest result");
  expect(
    within(screen.getByLabelText("Volume usage summary")).getByText(
      /200 bytes/,
    ),
  ).toBeDefined();
  expect(requests.at(-1)).toMatchObject({
    resultVersion: "v2",
    target: { volumeId: "AQ" },
  });
  expect(details).toEqual(["v1", "v2"]);

  fireEvent.click(
    screen.getByRole("button", { name: "Hide volume explanation" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  await screen.findByLabelText("Volume usage summary");
  expect(screen.queryByText("Updated to the latest result")).toBeNull();
  expect(requests.at(-1)).toMatchObject({ resultVersion: "v2" });
});
