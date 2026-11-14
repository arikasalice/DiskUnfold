import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AnalysisPanel } from "../src/features/analysis/AnalysisPanel";
import { windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  ScopeReview,
  SpaceTotal,
  TargetList,
} from "../src/ipc";

/*
 * F02 FR8.HD1: "Browse folders" in the analysis panel names the scan whose
 * result it opens, and a newly started scan is never paired with the result
 * of the scan before it.
 */

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
      access: "accessible",
      reasonKey: null,
    },
  ],
  probeJobRef: "probe",
  observedAt: timestamp,
  unavailableReason: null,
};

type Job = { state: AnalysisJobStatus["state"]; version: string };

function deferred() {
  let release = () => {};
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

/**
 * A client whose scans report the given state and result. `gates` holds a
 * scan.start by its order or a space.total by its result version until the
 * test releases it.
 */
function scanClient(
  jobs: Record<string, Job>,
  order: string[],
  gates: Record<string, Promise<void>> = {},
) {
  let starts = 0;
  const client: AnalysisClient = {
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
        case "scan.start": {
          const start = starts++;
          await gates[`start:${start}`];
          return {
            jobRef: order[start],
            existing: false,
            scopeVersion: "scope",
            scopeFingerprint: "verified",
          };
        }
        case "job.list":
          return [];
        case "job.get": {
          const { jobRef } = body as { jobRef: string };
          const job = jobs[jobRef]!;
          const ended = job.state === "done";
          return {
            jobRef,
            kind: "scan.metadata",
            state: job.state,
            sequence: 3,
            resultVersion: ended ? job.version : null,
            reasonKey: null,
            ...(ended
              ? {}
              : {
                  scanResult: {
                    lineageId: `lineage-${jobRef}`,
                    sequence: 1,
                    resultVersion: job.version,
                  },
                }),
          } satisfies AnalysisJobStatus;
        }
        case "space.total": {
          const request = body as { requestSeq: number; resultVersion: string };
          await gates[`total:${request.resultVersion}`];
          return {
            resultVersion: request.resultVersion,
            requestSeq: request.requestSeq,
            completeness: "missing",
            sizes: [],
            missingItems: "0",
            inaccessibleRanges: null,
            untraversedLinks: "0",
          } satisfies SpaceTotal;
        }
        default:
          throw new Error(`Unavailable ${name}`);
      }
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  };
  return client;
}

function renderPanel(client: AnalysisClient) {
  const onBrowse = vi.fn();
  render(
    <AnalysisPanel
      client={client}
      preferences={{
        display: { system: "iec", unit: "auto" },
        basis: "logical",
        warning: undefined,
      }}
      onBrowse={onBrowse}
    />,
  );
  return onBrowse;
}

async function startScan() {
  const start = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Start scan",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
}

async function startFirstScan() {
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await startScan();
}

const browseButton = () =>
  screen.queryByRole("button", { name: "Browse folders" });

test.each([
  ["running", "Pause scan"],
  ["paused", "Resume scan"],
] as const)(
  "F02 FR8.HD1: Browse folders on a %s scan names that scan and its shown result",
  async (state, control) => {
    const onBrowse = renderPanel(
      scanClient({ "scan-a": { state, version: "part-a" } }, ["scan-a"]),
    );
    await startFirstScan();
    await screen.findByRole("button", { name: control });
    fireEvent.click(
      await screen.findByRole("button", { name: "Browse folders" }),
    );
    expect(onBrowse).toHaveBeenCalledExactlyOnceWith(
      "part-a",
      undefined,
      "scan-a",
    );
  },
);

// Every total the panel offers comes from a scan it started, so it always
// names one; an ended scan still names itself. The folder list shows no
// newer-result bar for it because the panel withdraws the scan context once
// the scan stops running.
test("F02 FR8.HD1: Browse folders on an ended scan still names that scan", async () => {
  const onBrowse = renderPanel(
    scanClient({ "scan-a": { state: "done", version: "result-a" } }, [
      "scan-a",
    ]),
  );
  await startFirstScan();
  await screen.findByRole("button", { name: "Analyze again" });
  fireEvent.click(
    await screen.findByRole("button", { name: "Browse folders" }),
  );
  expect(onBrowse).toHaveBeenCalledExactlyOnceWith(
    "result-a",
    undefined,
    "scan-a",
  );
});

test("F02 FR8.HD1: a new scan is never paired with the previous scan's result", async () => {
  const secondStart = deferred();
  const secondTotal = deferred();
  const onBrowse = renderPanel(
    scanClient(
      {
        "scan-a": { state: "done", version: "result-a" },
        "scan-b": { state: "running", version: "result-b" },
      },
      ["scan-a", "scan-b"],
      { "start:1": secondStart.promise, "total:result-b": secondTotal.promise },
    ),
  );
  await startFirstScan();
  fireEvent.click(await screen.findByRole("button", { name: "Analyze again" }));
  await startScan();

  // scan.start is still in flight: the panel keeps the earlier scan and its result.
  fireEvent.click(browseButton()!);
  expect(onBrowse).toHaveBeenLastCalledWith("result-a", undefined, "scan-a");

  // The new receipt renders before the new scan's first total is read. The
  // earlier total is stamped with the earlier scan, so nothing is offered.
  await act(async () => secondStart.release());
  await screen.findByRole("button", { name: "Pause scan" });
  expect(browseButton()).toBeNull();

  await act(async () => secondTotal.release());
  await waitFor(() => expect(browseButton()).not.toBeNull());
  fireEvent.click(browseButton()!);
  expect(onBrowse).toHaveBeenLastCalledWith("result-b", undefined, "scan-b");
  expect(onBrowse).toHaveBeenCalledTimes(2);
});
