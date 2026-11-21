import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { AnalysisPanel } from "../src/features/analysis/AnalysisPanel";
import { ScanLaneProgress } from "../src/features/analysis/ScanLaneProgress";
import { ScanResourceContext } from "../src/features/analysis/scan-resource-context";
import { ScopeCollectionConditions } from "../src/features/analysis/ResourceControls";
import { AnalysisError, windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  ScanCollection,
  ScanLaneStatus,
  ScanSetResourceRequest,
  ScanSetResourceResponse,
  ScopeDraft,
  ScopeReview,
  TargetList,
} from "../src/ipc";

afterEach(cleanup);

const timestamp = "2026-11-21T00:00:00.000Z";
const path = windowsPath("C:\\fixture");
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

function scopeClient(
  start: (request: unknown, attempt: number) => unknown = () => ({
    jobRef: "scan",
    existing: false,
    scopeVersion: "scope",
    scopeFingerprint: "reviewed",
    collection: { resourceMode: "auto" },
  }),
  jobState: AnalysisJobStatus["state"] = "running",
) {
  const starts: unknown[] = [];
  const client = {
    call: (async (name: string, body: unknown) => {
      switch (name) {
        case "target.list":
          return targets;
        case "scope.review": {
          const request = body as {
            draft: ScopeDraft;
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
            reviewFingerprint: "reviewed",
            observedAt: timestamp,
            requestSeq: request.requestSeq,
          } satisfies ScopeReview;
        }
        case "scan.start":
          starts.push(body);
          return start(body, starts.length);
        case "job.get":
          return {
            jobRef: "scan",
            kind: "scan.metadata",
            state: jobState,
            sequence: 1,
            resultVersion: null,
            reasonKey: null,
          } satisfies AnalysisJobStatus;
        case "job.list":
          return [];
        default:
          throw new Error(`Unavailable ${name}`);
      }
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  } satisfies AnalysisClient;
  return { client, starts };
}

async function prepareScopeStart() {
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start analysis",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  return start;
}

function openScopeConditions() {
  fireEvent.click(
    screen.getByRole("button", { name: "Change collection conditions" }),
  );
}

function openScopeLimit() {
  fireEvent.click(
    screen.getByRole("button", {
      name: /Advanced.*Concurrency limit/,
    }),
  );
  return screen.getByLabelText("Concurrency limit");
}

test("scope conditions are collapsed and identify the default", () => {
  const { client } = scopeClient();
  render(<AnalysisPanel client={client} preferences={preferences} />);

  expect(
    screen.getByText(
      (_content, element) =>
        element?.textContent === "Resource use: Auto (default)",
    ),
  ).toBeDefined();
  expect(screen.queryByRole("radiogroup", { name: "Resource use" })).toBeNull();
  const disclosure = screen.getByRole("button", {
    name: "Change collection conditions",
  });
  expect(disclosure.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(disclosure);
  expect(
    screen.getByRole("radiogroup", { name: "Resource use" }),
  ).toBeDefined();
});

test("scope conditions follow the sample entry and sit above the action bar", () => {
  const { client } = scopeClient();
  render(
    <AnalysisPanel
      client={client}
      preferences={preferences}
      startAlternative={<div data-testid="sample-entry">Sample experience</div>}
    />,
  );

  const sample = screen.getByTestId("sample-entry");
  const body = sample.closest(".du-target-editor__body");
  const conditions = body?.querySelector(".du-collection-conditions");
  expect(conditions).not.toBeNull();
  expect(body?.lastElementChild).toBe(conditions);
  expect(
    body?.nextElementSibling?.classList.contains("du-target-editor__footer"),
  ).toBe(true);
});

test("quiet mode and a limit are sent with scan.start", async () => {
  const { client, starts } = scopeClient();
  render(<AnalysisPanel client={client} preferences={preferences} />);
  const start = await prepareScopeStart();
  openScopeConditions();
  fireEvent.click(screen.getByRole("radio", { name: "Quiet" }));
  fireEvent.change(openScopeLimit(), { target: { value: "3" } });
  fireEvent.click(start);

  await waitFor(() => expect(starts).toHaveLength(1));
  expect(starts[0]).toMatchObject({
    collection: { resourceMode: "quiet", userLimit: 3 },
  });
});

test("a collapsed summary includes the selected mode and limit", async () => {
  const { client } = scopeClient();
  render(<AnalysisPanel client={client} preferences={preferences} />);
  await prepareScopeStart();
  openScopeConditions();
  fireEvent.click(screen.getByRole("radio", { name: "Fast" }));
  fireEvent.change(openScopeLimit(), { target: { value: "8" } });
  fireEvent.click(
    screen.getByRole("button", { name: "Hide collection conditions" }),
  );

  expect(
    screen.getByText("Resource use: Fast · Up to 8 at once"),
  ).toBeDefined();
});

test("default scope conditions are omitted from scan.start", async () => {
  const { client, starts } = scopeClient();
  render(<AnalysisPanel client={client} preferences={preferences} />);
  fireEvent.click(await prepareScopeStart());

  await waitFor(() => expect(starts).toHaveLength(1));
  expect(starts[0]).not.toHaveProperty("collection");
});

test("a locally invalid scope limit shows its fallback and does not block starting", async () => {
  const { client, starts } = scopeClient();
  render(<AnalysisPanel client={client} preferences={preferences} />);
  const start = await prepareScopeStart();
  openScopeConditions();
  const limit = openScopeLimit();
  fireEvent.change(limit, { target: { value: "0" } });

  expect(screen.getByText("Enter a whole number of 1 or more")).toBeDefined();
  expect(limit.getAttribute("aria-invalid")).toBe("true");
  expect(
    screen
      .getByText("Enter a whole number of 1 or more")
      .compareDocumentPosition(
        screen.getByText("Using the previous value (automatic)"),
      ) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    screen.getByText("Using the previous value (automatic)"),
  ).toBeDefined();
  expect(start.disabled).toBe(false);
  fireEvent.click(start);

  await waitFor(() => expect(starts).toHaveLength(1));
  expect(starts[0]).not.toHaveProperty("collection");
  expect(screen.getByText("Enter a whole number of 1 or more")).toBeDefined();
});

test("scan.start limit refusal retries once without the prior automatic limit", async () => {
  const { client, starts } = scopeClient((_request, attempt) => {
    if (attempt === 1)
      throw new AnalysisError(
        "INVALID_ARGUMENT",
        "scan.resource_limit_invalid",
      );
    return {
      jobRef: "scan",
      existing: false,
      scopeVersion: "scope",
      scopeFingerprint: "reviewed",
      collection: { resourceMode: "fast" },
    };
  });
  render(<AnalysisPanel client={client} preferences={preferences} />);
  const start = await prepareScopeStart();
  openScopeConditions();
  fireEvent.click(screen.getByRole("radio", { name: "Fast" }));
  fireEvent.change(openScopeLimit(), { target: { value: "40" } });
  fireEvent.click(start);

  await waitFor(() => expect(starts).toHaveLength(2));
  expect(starts[0]).toMatchObject({
    collection: { resourceMode: "fast", userLimit: 40 },
  });
  expect(starts[1]).toMatchObject({
    collection: { resourceMode: "fast" },
  });
  expect(starts[1]).not.toHaveProperty("collection.userLimit");
  expect(
    await screen.findByText("Enter a whole number of 1 or more"),
  ).toBeDefined();
  expect(
    screen.getByText("Using the previous value (automatic)"),
  ).toBeDefined();
});

test("scan.start retries automatically when the previous limit is refused", async () => {
  const { client, starts } = scopeClient((_request, attempt) => {
    if (attempt === 1)
      return {
        jobRef: "scan",
        existing: false,
        scopeVersion: "scope",
        scopeFingerprint: "reviewed",
        collection: { resourceMode: "fast", userLimit: 30 },
      };
    if (attempt < 4)
      throw new AnalysisError(
        "INVALID_ARGUMENT",
        "scan.resource_limit_invalid",
      );
    return {
      jobRef: "scan",
      existing: false,
      scopeVersion: "scope",
      scopeFingerprint: "reviewed",
      collection: { resourceMode: "fast" },
    };
  }, "done");
  render(<AnalysisPanel client={client} preferences={preferences} />);
  fireEvent.click(await prepareScopeStart());
  await waitFor(() => expect(starts).toHaveLength(1));

  fireEvent.click(await screen.findByRole("button", { name: "Analyze again" }));
  const start = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Start analysis",
  });
  openScopeConditions();
  fireEvent.change(openScopeLimit(), { target: { value: "40" } });
  fireEvent.click(start);

  await waitFor(() => expect(starts).toHaveLength(4));
  expect(starts[1]).toMatchObject({
    collection: { resourceMode: "fast", userLimit: 40 },
  });
  expect(starts[2]).toMatchObject({
    collection: { resourceMode: "fast", userLimit: 30 },
  });
  expect(starts[3]).toMatchObject({ collection: { resourceMode: "fast" } });
  expect(starts[3]).not.toHaveProperty("collection.userLimit");
  expect(
    await screen.findByText("Enter a whole number of 1 or more"),
  ).toBeDefined();
  expect(
    screen.getByText("Using the previous value (automatic)"),
  ).toBeDefined();
});

const laneStatus: ScanLaneStatus = {
  sequence: 1,
  observedAt: timestamp,
  lanes: [
    {
      laneIndex: 0,
      rootIndexes: [0],
      inFlight: 1,
      limit: 2,
      mode: "auto",
      permit: 4,
      waitReason: "none",
      waitConfirmed: true,
      remainingDirectories: "4",
      failures: "0",
    },
  ],
};
const roots = [windowsPath("C:\\fixture")];

function setResourceResponse(
  outcome: ScanSetResourceResponse["outcome"],
  effectiveLimit = 3,
): ScanSetResourceResponse {
  return {
    outcome,
    lanes: [{ laneIndex: 0, effectiveLimit }],
    appliesFrom: "next_assignment",
  };
}

function renderProgress(
  client: AnalysisClient,
  collection: ScanCollection = { resourceMode: "auto" },
  status = laneStatus,
) {
  return render(
    <ScanResourceContext.Provider
      value={{ client, jobRef: "scan", collection, active: true }}
    >
      <ScanLaneProgress jobRef="scan" status={status} roots={roots} />
    </ScanResourceContext.Provider>,
  );
}

function resourceClient(
  call: (request: ScanSetResourceRequest) => Promise<ScanSetResourceResponse>,
) {
  const requests: ScanSetResourceRequest[] = [];
  const client = {
    call: (async (name: string, body: unknown) => {
      if (name !== "scan.set_resource") throw new Error(`Unavailable ${name}`);
      requests.push(body as ScanSetResourceRequest);
      return call(body as ScanSetResourceRequest);
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  } satisfies AnalysisClient;
  return { client, requests };
}

test("progress changes use full replacement, show acceptance, and announce lane application once", async () => {
  let resolveChange!: (response: ScanSetResourceResponse) => void;
  const { client, requests } = resourceClient(
    () =>
      new Promise((resolve) => {
        resolveChange = resolve;
      }),
  );
  const { rerender, container } = renderProgress(client, {
    resourceMode: "quiet",
    userLimit: 2,
  });
  const fast = await screen.findByRole("radio", { name: "Fast" });
  fireEvent.click(fast);
  expect(screen.getByText("Applying")).toBeDefined();
  expect(
    await screen.findByText("Takes effect after the current folder"),
  ).toBeDefined();
  expect(requests[0]).toEqual({
    jobRef: "scan",
    resourceMode: "fast",
    userLimit: 2,
  });

  await act(async () => resolveChange(setResourceResponse("accepted", 3)));
  expect(
    screen.getAllByText("Takes effect after the current folder").length,
  ).toBe(1);
  rerender(
    <ScanResourceContext.Provider
      value={{
        client,
        jobRef: "scan",
        collection: { resourceMode: "quiet" },
        active: true,
      }}
    >
      <ScanLaneProgress
        jobRef="scan"
        status={{
          ...laneStatus,
          sequence: 2,
          lanes: [{ ...laneStatus.lanes[0]!, mode: "fast", limit: 3 }],
        }}
        roots={roots}
      />
    </ScanResourceContext.Provider>,
  );
  const announcement = await screen.findByText("Resource use changed to Fast");
  expect(announcement.closest(".du-announcer")).not.toBeNull();
  expect(
    container.querySelector(".du-scan-resource-control__feedback"),
  ).toBeNull();
  expect(
    screen.queryByText("Takes effect after the current folder"),
  ).toBeNull();
});

test("the progress limit disclosure starts collapsed", async () => {
  const { client } = resourceClient(async () =>
    setResourceResponse("accepted", 2),
  );
  renderProgress(client);

  const disclosure = await screen.findByRole("button", { name: /Advanced/ });
  expect(disclosure.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByLabelText("Concurrency limit")).toBeNull();
});

test("clearing the progress limit omits userLimit and no_effect keeps the choice", async () => {
  const { client, requests } = resourceClient(async () =>
    setResourceResponse("no_effect"),
  );
  renderProgress(client, { resourceMode: "quiet", userLimit: 3 });
  fireEvent.click(await screen.findByRole("button", { name: /Advanced/ }));
  fireEvent.change(screen.getByLabelText("Concurrency limit"), {
    target: { value: "" },
  });
  await waitFor(() => expect(requests).toHaveLength(1));
  expect(requests[0]).toEqual({ jobRef: "scan", resourceMode: "quiet" });
  expect(screen.getByRole("radio", { name: "Quiet" })).toBeDefined();
  expect(screen.getByLabelText("Concurrency limit").getAttribute("max")).toBe(
    "4",
  );
});

test("resource_limit_invalid retries with the previous value and shows its range", async () => {
  let calls = 0;
  const { client, requests } = resourceClient(async () => {
    calls += 1;
    if (calls === 1)
      throw new AnalysisError(
        "INVALID_ARGUMENT",
        "scan.resource_limit_invalid",
      );
    return setResourceResponse("accepted", 2);
  });
  renderProgress(client);
  fireEvent.click(await screen.findByRole("button", { name: /Advanced/ }));
  fireEvent.change(screen.getByLabelText("Concurrency limit"), {
    target: { value: "3" },
  });
  await waitFor(() => expect(requests).toHaveLength(2));
  expect(requests[0]).toEqual({
    jobRef: "scan",
    resourceMode: "auto",
    userLimit: 3,
  });
  expect(requests[1]).toEqual({ jobRef: "scan", resourceMode: "auto" });
  expect(await screen.findByText("This storage accepts 1 to 4")).toBeDefined();
  expect(
    screen.getByText("Using the previous value (automatic)"),
  ).toBeDefined();
});

test("a locally invalid progress limit does not block a mode change", async () => {
  const { client, requests } = resourceClient(async () =>
    setResourceResponse("accepted", 3),
  );
  renderProgress(client);
  fireEvent.click(await screen.findByRole("button", { name: /Advanced/ }));
  fireEvent.change(screen.getByLabelText("Concurrency limit"), {
    target: { value: "0" },
  });
  fireEvent.click(screen.getByRole("radio", { name: "Fast" }));

  await waitFor(() => expect(requests).toHaveLength(1));
  expect(requests[0]).toEqual({ jobRef: "scan", resourceMode: "fast" });
  expect(screen.getByText("This storage accepts 1 to 4")).toBeDefined();
  expect(
    screen.getByText("Using the previous value (automatic)"),
  ).toBeDefined();
});

test("a mode change during a pending limit uses the last accepted limit after invalid input", async () => {
  const resolvers: ((value: ScanSetResourceResponse) => void)[] = [];
  const { client, requests } = resourceClient(
    () =>
      new Promise((resolve) => {
        resolvers.push(resolve);
      }),
  );
  renderProgress(client, { resourceMode: "quiet", userLimit: 2 });
  fireEvent.click(await screen.findByRole("button", { name: /Advanced/ }));
  const limit = screen.getByLabelText("Concurrency limit");
  fireEvent.change(limit, { target: { value: "3" } });
  fireEvent.change(limit, { target: { value: "0" } });
  fireEvent.click(screen.getByRole("radio", { name: "Fast" }));

  expect(requests).toEqual([
    { jobRef: "scan", resourceMode: "quiet", userLimit: 3 },
    { jobRef: "scan", resourceMode: "fast", userLimit: 2 },
  ]);
  await act(async () => resolvers[0]!(setResourceResponse("accepted", 3)));
  await act(async () => resolvers[1]!(setResourceResponse("accepted", 2)));
  expect(screen.getByText("Using the previous value (2)")).toBeDefined();
});

test("already_terminal and job.not_found quietly hide the progress control", async () => {
  for (const failure of [
    setResourceResponse("already_terminal"),
    new AnalysisError("NOT_FOUND", "job.not_found"),
  ]) {
    cleanup();
    const { client } = resourceClient(async () => {
      if (failure instanceof Error) throw failure;
      return failure;
    });
    renderProgress(client);
    fireEvent.click(await screen.findByRole("radio", { name: "Fast" }));
    await waitFor(() =>
      expect(screen.queryByRole("radio", { name: "Fast" })).toBeNull(),
    );
  }
});

test("stale progress responses cannot replace a newer resource choice", async () => {
  const resolvers: ((value: ScanSetResourceResponse) => void)[] = [];
  const { client } = resourceClient(
    () =>
      new Promise((resolve) => {
        resolvers.push(resolve);
      }),
  );
  renderProgress(client);
  fireEvent.click(await screen.findByRole("radio", { name: "Quiet" }));
  fireEvent.click(screen.getByRole("radio", { name: "Fast" }));
  expect(resolvers).toHaveLength(2);

  await act(async () => resolvers[1]!(setResourceResponse("accepted", 3)));
  await act(async () => resolvers[0]!(setResourceResponse("accepted", 1)));
  expect(
    screen
      .getByRole("radio", { name: "Fast" })
      .closest("label")
      ?.getAttribute("data-selected"),
  ).not.toBeNull();
  expect(
    screen.getAllByText("Takes effect after the current folder"),
  ).toHaveLength(1);
});

const preferences = {
  display: { system: "iec" as const, unit: "auto" as const },
  basis: "logical" as const,
  warning: undefined,
};

test("a refused limit is not named as the value in use", () => {
  render(
    <ScopeCollectionConditions
      mode="fast"
      userLimit={12}
      draft="12"
      error="rejected"
      previousValue={4}
      disabled={false}
      max={6}
      defaultExpanded
      onModeChange={() => {}}
      onLimitChange={() => {}}
      onReset={() => {}}
    />,
  );

  expect(
    screen.getByText("Resource use: Fast · Up to 4 at once"),
  ).toBeDefined();
  expect(screen.queryByText(/Up to 12/)).toBeNull();
  expect(
    screen.getByRole("button", { name: /Advanced · Concurrency limit \(4\)/ }),
  ).toBeDefined();
});
