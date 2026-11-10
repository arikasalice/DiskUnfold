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
import { afterEach, expect, test } from "vitest";
import { App } from "../src/app/App";
import { AnalysisError, createAnalysisClient, windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  BrowseDetail,
  EventEnvelope,
  SpaceTotal,
  TargetList,
  TargetProbeEvent,
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
}: { stale?: boolean; earlyProbe?: EventEnvelope; missingJob?: boolean } = {}) {
  const listeners = new Set<(event: EventEnvelope) => void>();
  const calls: { name: string; body: unknown }[] = [];
  const client: AnalysisClient = {
    call: (async (name: string, body: unknown) => {
      calls.push({ name, body });
      switch (name) {
        case "target.list":
          if (earlyProbe)
            for (const listener of listeners) listener(earlyProbe);
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
          if (missingJob) throw new AnalysisError("NOT_FOUND", "job.not_found");
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
            untraversedLinks: "2",
          };
          return result;
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
  act(() => {
    window.dispatchEvent(
      new CustomEvent("du://folder-drop", { detail: { path: dropped } }),
    );
  });
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
  act(() => {
    window.dispatchEvent(
      new CustomEvent("du://folder-drop", {
        detail: { reason: "invalid_target" },
      }),
    );
  });
  expect(screen.getByText("Drop one accessible local folder.")).toBeDefined();
  expect(
    (
      screen.getByRole("textbox", {
        name: "Windows folder path",
      }) as HTMLInputElement
    ).value,
  ).toBe(path.display);
});
