import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "../src/app/App";
import { windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  EventEnvelope,
  ScopeReview,
  SpaceTotal,
} from "../src/ipc";

afterEach(cleanup);
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

test("F02 FR3.HD1.gui-web.DD1: the analysis screen polls at 500 ms and stops after terminal status", async () => {
  vi.useFakeTimers({
    toFake: [
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "Date",
    ],
  });

  const pollStarts: number[] = [];
  const setInterval = window.setInterval.bind(window);
  vi.spyOn(window, "setInterval").mockImplementation(((
    handler: TimerHandler,
    timeout?: number,
    ...args: unknown[]
  ) => {
    if (timeout === 500) pollStarts.push(Date.now());
    return setInterval(handler, timeout, ...args);
  }) as typeof window.setInterval);

  const frames = new Map<number, number>();
  let nextFrame = 0;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    const frame = ++nextFrame;
    const timer = window.setTimeout(() => {
      frames.delete(frame);
      callback(Date.now());
    }, 0);
    frames.set(frame, timer);
    return frame;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((frame) => {
    const timer = frames.get(frame);
    if (timer !== undefined) window.clearTimeout(timer);
    frames.delete(frame);
  });

  const path = windowsPath("C:\\fixture");
  const calls: { name: string; body: unknown }[] = [];
  const listeners = new Set<(event: EventEnvelope) => void>();
  let latestVersion = "v1";
  let finishScan = false;
  let jobGets = 0;
  const scanReads = () =>
    calls.filter(
      ({ name, body }) =>
        name === "job.get" && (body as { jobRef: string }).jobRef === "scan",
    ).length;
  const totalVersions = () =>
    calls
      .filter(({ name }) => name === "space.total")
      .map(({ body }) => (body as { resultVersion: string }).resultVersion);

  const total = (version: string, requestSeq: number): SpaceTotal => ({
    resultVersion: version,
    requestSeq,
    completeness: "missing",
    sizes: [
      {
        size: {
          basis: "logical",
          evidence: "os_reported",
          state: "lower_bound",
          bytes: version === "v1" ? "42" : "84",
        },
        completeness: "missing",
        measurementLimits: [],
      },
      {
        size: { basis: "allocated", evidence: "unknown", state: "unknown" },
        completeness: "missing",
        measurementLimits: ["space.data_insufficient"],
      },
    ],
    missingItems: "1",
    inaccessibleRanges: null,
    untraversedLinks: "2",
  });

  const status = (): AnalysisJobStatus => {
    if (finishScan) {
      return {
        jobRef: "scan",
        kind: "scan.metadata",
        state: "done",
        sequence: 5,
        resultVersion: "v2",
        reasonKey: null,
        scanResult: { lineageId: "lineage", sequence: 2, resultVersion: "v2" },
        terminal: {
          state: "done",
          sequence: 5,
          completed: 1,
          overwritten: 0,
          failed: 0,
          skipped: 0,
          unprocessed: 0,
          historyRecorded: true,
          resultVersion: "v2",
        },
      };
    }
    const version = jobGets === 1 ? "v1" : latestVersion;
    return {
      jobRef: "scan",
      kind: "scan.metadata",
      state: "running",
      sequence: version === "v1" ? 3 : 4,
      resultVersion: version,
      reasonKey: null,
      scanResult: {
        lineageId: "lineage",
        sequence: version === "v1" ? 1 : 2,
        resultVersion: version,
      },
    };
  };

  const client = {
    async call(name: string, body: unknown): Promise<unknown> {
      calls.push({ name, body });
      switch (name) {
        case "space.display_preferences":
          return {
            settingsRevision: "0",
            unitSystem: "iec",
            unit: "auto",
            defaultBasis: "logical",
            warningKey: null,
          };
        case "target.list":
          return {
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
            probeJobRef: null,
            observedAt: "2026-11-10T00:00:00.000Z",
            unavailableReason: null,
          };
        case "scope.review": {
          const request = body as {
            draft: ScopeReview["scope"];
            requestSeq: number;
          };
          return {
            scope: request.draft,
            notices: [],
            targets: request.draft.includes.map((target) => ({
              path: target,
              access: "accessible",
              reasonKey: null,
            })),
            reviewFingerprint: "verified",
            observedAt: "2026-11-10T00:00:00.000Z",
            requestSeq: request.requestSeq,
          };
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
          return [
            {
              jobRef: "scan",
              kind: "scan.metadata",
              scopeSummary: "scope",
              state: "running",
              waitReason: "unknown",
              priority: "normal",
              supportedControls: ["cancel"],
              sequence: 3,
            },
          ];
        case "job.get":
          if ((body as { jobRef: string }).jobRef === "scan") jobGets += 1;
          return status();
        case "space.total": {
          const request = body as { resultVersion: string; requestSeq: number };
          return total(request.resultVersion, request.requestSeq);
        }
        default:
          throw new Error(`Unexpected analysis command ${name}`);
      }
    },
    subscribe(listener: (event: EventEnvelope) => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  } as unknown as AnalysisClient;

  async function advance(milliseconds: number) {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(milliseconds);
    });
  }

  render(<App analysis={client} />);
  await advance(0);
  fireEvent.click(screen.getByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await advance(0);
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start analysis",
  });
  expect(start.disabled).toBe(false);
  fireEvent.click(start);
  await advance(20);

  expect(screen.getByText("At least 42 bytes")).toBeDefined();
  expect(pollStarts).toHaveLength(1);
  const pollStart = pollStarts[0]!;
  const initialReads = scanReads();
  expect(initialReads).toBeGreaterThan(0);

  latestVersion = "v2";
  const untilBeforeTick = pollStart + 499 - Date.now();
  expect(untilBeforeTick).toBeGreaterThanOrEqual(0);
  await advance(untilBeforeTick);
  const beforeTickReads = scanReads();
  expect(beforeTickReads).toBe(initialReads);

  await advance(1);
  const atTickReads = scanReads();
  expect(atTickReads).toBeGreaterThan(beforeTickReads);
  expect(totalVersions()).toEqual(["v1"]);

  await advance(500);
  expect(totalVersions()).toContain("v2");
  expect(screen.getByText("At least 84 bytes")).toBeDefined();

  finishScan = true;
  const beforeFinishReads = scanReads();
  await advance(500);
  expect(scanReads()).toBeGreaterThan(beforeFinishReads);
  expect(screen.getByText("Scan finished")).toBeDefined();

  const afterFinishReads = scanReads();
  await advance(1_500);
  expect(scanReads()).toBe(afterFinishReads);
});
