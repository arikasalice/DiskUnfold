import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { App } from "../src/app/App";
import { AnalysisError, windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  EventEnvelope,
  SpaceTotal,
  TargetList,
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

function fixture(stale = false) {
  const listeners = new Set<(event: EventEnvelope) => void>();
  const calls: { name: string; body: unknown }[] = [];
  const client: AnalysisClient = {
    call: (async (name: string, body: unknown) => {
      calls.push({ name, body });
      switch (name) {
        case "target.list":
          return targets;
        case "scope.review": {
          const request = body as { draft: unknown; requestSeq: number };
          return {
            scope: request.draft,
            notices: [],
            targets: [{ path, access: "accessible", reasonKey: null }],
            reviewFingerprint: "verified",
            observedAt: timestamp,
            requestSeq: request.requestSeq,
          };
        }
        case "scan.start":
          if (stale) throw new AnalysisError("STALE_VERSION", "scope.changed");
          return {
            jobRef: "scan",
            existing: false,
            scopeVersion: "scope",
            scopeFingerprint: "verified",
          };
        case "job.get":
          return {
            jobRef: "scan",
            kind: "scan.metadata",
            state: "partial",
            sequence: 3,
            resultVersion: "result",
            reasonKey: null,
          };
        case "space.total": {
          const request = body as { requestSeq: number };
          const result: SpaceTotal = {
            resultVersion: "result",
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
            untraversedLinks: null,
          };
          return result;
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
  expect(calls.find((call) => call.name === "scan.start")?.body).toMatchObject({
    reviewFingerprint: "verified",
  });
  expect(calls.find((call) => call.name === "space.total")?.body).toMatchObject(
    { resultVersion: "result", target: { kind: "scope_root" } },
  );
});

test("F01: stale scope disables start and requires review again", async () => {
  const { client, calls } = fixture(true);
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
