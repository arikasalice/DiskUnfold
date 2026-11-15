import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { Profiler, useMemo } from "react";
import { expect, vi } from "vitest";
import { AnalysisPanel } from "../src/features/analysis/AnalysisPanel";
import { VolumeExplanation } from "../src/features/analysis/VolumeExplanation";
import { useSummaryBatch } from "../src/features/analysis/summary-batch";
import type { SizeDisplay } from "../src/features/analysis/space-format";
import { AnalysisError, windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  BrowseDetail,
  EventEnvelope,
  JobState,
  OsPath,
  ScopeReview,
  SpaceTotal,
  TargetList,
  VolumeGapResponse,
} from "../src/ipc";
import { RESULT_REFLECT_INTERVAL_MS } from "../src/state/result-cadence";

/*
 * Shared fixtures for the analysis summary's automatic reflection
 * (F02 FR3.HD1.gui-web.DD1): a live scan whose results name their version
 * in every value, the panel started on it, and the summary batch on its own.
 */

let reflectOffset = 0;

export function resetReflectOffset() {
  reflectOffset = 0;
}

/** Moves the cadence clock one interval ahead so the next publication is read at once. */
export function passReflectInterval() {
  if (reflectOffset === 0) {
    const realNow = Date.now.bind(Date);
    vi.spyOn(Date, "now").mockImplementation(() => realNow() + reflectOffset);
  }
  reflectOffset += RESULT_REFLECT_INTERVAL_MS;
}

export const path = windowsPath("C:\\fixture");
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
export function amount(version: string) {
  return Number(version.slice(1));
}

export function totals(version: string, requestSeq: number, final: boolean) {
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

export function folderDetail(version: string): BrowseDetail {
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
export function liveScan() {
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
        case "space.volume_gap": {
          current(body.resultVersion);
          const { requestSeq } = body as unknown as { requestSeq: number };
          return { ...volumeReport(job.version), requestSeq };
        }
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

export async function startPanel(onRender: () => void = () => {}) {
  const scan = liveScan();
  render(
    <Profiler id="panel" onRender={onRender}>
      <AnalysisPanel
        client={scan.client}
        preferences={{
          display: { system: "iec", unit: "auto" },
          basis: "logical",
          warning: undefined,
        }}
      />
    </Profiler>,
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

export const logical = () =>
  within(screen.getByRole("group", { name: "Logical size" }));

export function coverage() {
  return within(screen.getByLabelText("Coverage limits"));
}

export function select(node: Node) {
  const range = document.createRange();
  range.selectNodeContents(node);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  act(() => {
    document.dispatchEvent(new Event("selectionchange"));
  });
}

export function clearSelection() {
  document.getSelection()!.removeAllRanges();
  act(() => {
    document.dispatchEvent(new Event("selectionchange"));
  });
}

export function volumeReport(version: string): VolumeGapResponse {
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

/** The number of bytes in the first "N bytes" of `node`, if any. */
function bytesIn(node: Element | null): number | undefined {
  const match = node?.textContent?.match(/(\d+) bytes/);
  return match ? Number(match[1]) : undefined;
}

/**
 * Records, at every React commit, the summary total and the value of the
 * volume part as the screen shows them. Pass `onRender` to a `Profiler`.
 */
export function watchSummary(summary: () => Element | null) {
  const seen: [number | undefined, number | undefined][] = [];
  return {
    seen,
    onRender: () => {
      seen.push([
        bytesIn(summary()),
        bytesIn(screen.queryByLabelText("Volume usage summary")),
      ]);
    },
    /** Commits that showed the part beside a total from another result. */
    mixed: () =>
      seen.filter(
        ([total, part]) =>
          total !== undefined && part !== undefined && total !== part,
      ),
  };
}

/** The summary batch on its own: its total, and the volume part beside it. */
export function SummaryHarness({
  client,
  version,
  hold = false,
  live = true,
  roots = [path],
  display = { system: "iec", unit: "auto" },
}: {
  client: AnalysisClient;
  version: string;
  hold?: boolean;
  live?: boolean;
  roots?: readonly OsPath[];
  display?: SizeDisplay;
}) {
  const total = useMemo(() => totals(version, 0, !live), [version, live]);
  const summary = useSummaryBatch(client, total, hold, live);
  const size = summary.total?.sizes[0]?.size;
  return (
    <>
      <section aria-label="Summary total">
        {size && "bytes" in size ? `${size.bytes} bytes` : ""}
      </section>
      <VolumeExplanation
        volume={summary.volume}
        roots={roots}
        display={display}
      />
    </>
  );
}
