import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { AnalysisPanel } from "../src/features/analysis/AnalysisPanel";
import { windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  JobState,
  ScopeReview,
  TargetList,
} from "../src/ipc";

/*
 * #174: the progress text in the analysis header follows the same state as
 * its badge. While the scan waits to start, both say it waits; once it runs,
 * the progress text says the folder is being scanned.
 */

afterEach(cleanup);

const path = windowsPath("C:\\fixture");
const timestamp = "2026-11-20T00:00:00.000Z";
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

/** A scan whose job state the test sets; it has no result yet. */
function waitingClient(job: { state: JobState }): AnalysisClient {
  return {
    call: (async (name: string, body: unknown) => {
      switch (name) {
        case "target.list":
          return targets;
        case "scope.review": {
          const request = body as {
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
          };
        case "job.list":
          return [];
        case "job.get":
          return {
            jobRef: "scan",
            kind: "scan.metadata",
            state: job.state,
            sequence: 1,
            resultVersion: null,
            reasonKey: null,
          } satisfies AnalysisJobStatus;
        default:
          throw new Error(`Unavailable ${name}`);
      }
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  };
}

test("#174: while the scan waits to start, the progress text says so as the badge does", async () => {
  const job: { state: JobState } = { state: "queued" };
  render(
    <AnalysisPanel
      client={waitingClient(job)}
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
    name: "Start analysis",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);

  await screen.findByRole("progressbar", {
    name: "Waiting for the scan to start",
  });
  expect(screen.getByText("Waiting to scan")).toBeDefined();
  expect(
    screen.queryByRole("progressbar", { name: "Scanning folder" }),
  ).toBeNull();

  job.state = "running";
  await screen.findByRole(
    "progressbar",
    { name: "Scanning folder" },
    { timeout: 3000 },
  );
  expect(
    screen.queryByRole("progressbar", {
      name: "Waiting for the scan to start",
    }),
  ).toBeNull();
});
