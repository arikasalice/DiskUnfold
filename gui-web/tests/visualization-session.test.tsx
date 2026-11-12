import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, test } from "vitest";
import { createChartRenderers } from "../src/components";
import { VisualizationSession } from "../src/features/visualization/VisualizationSession";
import type { VisualizationViewRequest } from "../src/features/visualization/VisualizationSession";
import { fixtureText, modelFixture } from "../preview/model-fixture";
import { createAnalysisClient } from "../src/ipc";
import type { CallEnvelope, EventEnvelope } from "../src/ipc";

afterEach(cleanup);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function request(depthLimit = 1): VisualizationViewRequest {
  const frame = modelFixture().frame;
  return {
    resultRevision: frame.resultRevision,
    format: "treemap",
    aggregateScopeRef: frame.model.aggregateScopeRef,
    navigationScopeRef: frame.model.navigationScopeRef,
    measure: "size",
    sizeBasis: "logical",
    classificationCriterion: "type",
    colorCriterion: "type",
    denominatorBasis: "scope",
    viewport: { width: 960, height: 330, scale: 1 },
    visibleWindow: { left: 0, top: 0, right: 1_000_000, bottom: 1_000_000 },
    minAreaPx2: 1,
    depthLimit,
  };
}

function requestForRevision(resultRevision: string): VisualizationViewRequest {
  const view = request();
  return {
    ...view,
    resultRevision,
    aggregateScopeRef: {
      ...view.aggregateScopeRef,
      resultVersion: resultRevision,
    },
    navigationScopeRef: {
      ...view.navigationScopeRef,
      resultVersion: resultRevision,
    },
  };
}

function frameForRevision(resultRevision: string) {
  const frame = modelFixture().frame;
  frame.resultRevision = resultRevision;
  frame.model.aggregateScopeRef.resultVersion = resultRevision;
  frame.model.navigationScopeRef.resultVersion = resultRevision;
  for (const shape of frame.model.shapes) {
    if (shape.itemRef) shape.itemRef.resultVersion = resultRevision;
    if (shape.rootBranchRef) shape.rootBranchRef.resultVersion = resultRevision;
  }
  return frame;
}

function setup(
  transport: Parameters<typeof createAnalysisClient>[0],
  view = request(),
) {
  const client = createAnalysisClient(transport);
  client.subscribe = () => () => {};
  return {
    client,
    view,
    props: {
      client,
      request: view,
      canDisplay: () => true,
      text: fixtureText,
      autoLoadRows: false,
      resolveRenderer: createChartRenderers([]),
    },
  };
}

function groupMemberPage() {
  const fixture = modelFixture();
  const row = fixture.page.response.rows[0]!;
  return {
    ...fixture.page.response,
    rows: [
      { ...row, itemRef: null, groupRef: "group-a", displayName: "Group A" },
    ],
    totalCount: 1,
  };
}

function groupRowsResponse() {
  const fixture = modelFixture();
  const row = fixture.page.response.rows[0]!;
  return {
    rows: [{ ...row, groupRef: null, displayName: "Member file" }],
    totalCount: 1,
    groupTotal: row.size,
    sizeRange: { kind: "unknown" as const },
    resultRevision: fixture.frame.resultRevision,
    observedAt: fixture.frame.observedAt,
    completeness: fixture.frame.completeness,
  };
}

test("does not render before a delayed open and uses only the latest pending view", async () => {
  const opening = deferred<unknown>();
  const calls: CallEnvelope[] = [];
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    if (envelope.name === "viz.view.open") {
      return {
        name: envelope.name,
        correlationId: envelope.correlationId,
        contractVersion: envelope.contractVersion,
        body: await opening.promise,
      };
    }
    const fixture = modelFixture();
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body: fixture.frame,
    };
  });
  const rendered = render(<VisualizationSession {...props} />);
  expect(screen.getByRole("progressbar")).toBeTruthy();
  expect(calls.map((call) => call.name)).toEqual(["viz.view.open"]);

  const later = request(3);
  rendered.rerender(<VisualizationSession {...props} request={later} />);
  await act(async () => {
    opening.resolve({ viewChannel: "delayed-view" });
  });
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.render")).toBe(true),
  );
  const renders = calls.filter((call) => call.name === "viz.render");
  expect(renders).toHaveLength(1);
  expect(renders[0]!.body).toMatchObject({
    viewChannel: "delayed-view",
    requestSeq: 1,
    depthLimit: 3,
  });
});

test("a late open after unmount closes its own channel without rendering", async () => {
  const opening = deferred<unknown>();
  const calls: CallEnvelope[] = [];
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body: envelope.name === "viz.view.open" ? await opening.promise : {},
    };
  });
  const rendered = render(<VisualizationSession {...props} />);
  rendered.unmount();
  await act(async () => opening.resolve({ viewChannel: "late-open" }));
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.view.close")).toBe(true),
  );
  expect(calls.some((call) => call.name === "viz.render")).toBe(false);
  expect(
    calls
      .filter((call) => call.name === "viz.view.close")
      .map((call) => call.body),
  ).toEqual([{ viewChannel: "late-open" }]);
});

test("observer setup failure closes once and does not poison the next client session", async () => {
  const failedCalls: CallEnvelope[] = [];
  const successfulCalls: CallEnvelope[] = [];
  const first = setup(async (_, { envelope }) => {
    failedCalls.push(envelope);
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body:
        envelope.name === "viz.view.open"
          ? { viewChannel: "failed-observer" }
          : {},
    };
  });
  first.client.subscribe = () => {
    throw new Error("event subscription failed");
  };
  const rendered = render(<VisualizationSession {...first.props} />);
  await screen.findByText("Visualization could not be opened.");
  await waitFor(() =>
    expect(
      failedCalls.filter((call) => call.name === "viz.view.close"),
    ).toHaveLength(1),
  );
  expect(failedCalls.some((call) => call.name === "viz.render")).toBe(false);

  const next = setup(async (_, { envelope }) => {
    successfulCalls.push(envelope);
    const fixture = modelFixture();
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body:
        envelope.name === "viz.view.open"
          ? { viewChannel: "next-session" }
          : envelope.name === "viz.render"
            ? {
                ...fixture.frame,
                requestSeq: (envelope.body as { requestSeq: number })
                  .requestSeq,
              }
            : {},
    };
  });
  rendered.rerender(<VisualizationSession {...next.props} />);
  await waitFor(() =>
    expect(successfulCalls.some((call) => call.name === "viz.render")).toBe(
      true,
    ),
  );
  expect(
    failedCalls.filter((call) => call.name === "viz.view.close"),
  ).toHaveLength(1);
  expect(failedCalls.some((call) => call.name === "viz.cancel")).toBe(false);
  rendered.unmount();
  expect(
    failedCalls.filter((call) => call.name === "viz.view.close"),
  ).toHaveLength(1);
});

test("StrictMode effect replay closes the abandoned open and renders once", async () => {
  const calls: CallEnvelope[] = [];
  let openCount = 0;
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    const fixture = modelFixture();
    const body =
      envelope.name === "viz.view.open"
        ? { viewChannel: `strict-${++openCount}` }
        : envelope.name === "viz.render"
          ? {
              ...fixture.frame,
              requestSeq: (envelope.body as { requestSeq: number }).requestSeq,
            }
          : {};
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    };
  });
  render(
    <StrictMode>
      <VisualizationSession {...props} />
    </StrictMode>,
  );
  await waitFor(() =>
    expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(1),
  );
  expect(calls.filter((call) => call.name === "viz.view.open")).toHaveLength(2);
  await waitFor(() =>
    expect(
      calls.some(
        (call) =>
          call.name === "viz.view.close" &&
          (call.body as { viewChannel: string }).viewChannel === "strict-1",
      ),
    ).toBe(true),
  );
});

test("same-revision view changes share one channel and ignore late render responses", async () => {
  const calls: CallEnvelope[] = [];
  const oldRender = deferred<unknown>();
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    const fixture = modelFixture();
    if (
      envelope.name === "viz.render" &&
      (envelope.body as { requestSeq: number }).requestSeq === 1
    ) {
      return {
        name: envelope.name,
        correlationId: envelope.correlationId,
        contractVersion: envelope.contractVersion,
        body: await oldRender.promise,
      };
    }
    const body =
      envelope.name === "viz.view.open"
        ? { viewChannel: "one-view" }
        : envelope.name === "viz.render"
          ? {
              ...fixture.frame,
              requestSeq: (envelope.body as { requestSeq: number }).requestSeq,
            }
          : fixture.frame;
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    };
  });
  const rendered = render(<VisualizationSession {...props} />);
  await waitFor(() =>
    expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(1),
  );
  const changed = request(4);
  rendered.rerender(<VisualizationSession {...props} request={changed} />);
  await waitFor(() =>
    expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(2),
  );
  expect(calls.filter((call) => call.name === "viz.view.open")).toHaveLength(1);
  expect(
    calls
      .filter((call) => call.name === "viz.render")
      .map((call) => (call.body as { requestSeq: number }).requestSeq),
  ).toEqual([1, 2]);
  await act(async () => {
    oldRender.resolve({
      name: "viz.render",
      correlationId: calls.find((call) => call.name === "viz.render")!
        .correlationId,
      contractVersion: 1,
      body: modelFixture().frame,
    });
  });
});

test("in-place view edits and predicate identity changes do not replace the channel", async () => {
  const calls: CallEnvelope[] = [];
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    const fixture = modelFixture();
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body:
        envelope.name === "viz.view.open"
          ? { viewChannel: "stable-view" }
          : fixture.frame,
    };
  });
  const rendered = render(<VisualizationSession {...props} />);
  await waitFor(() =>
    expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(1),
  );
  props.request.depthLimit = 7;
  rendered.rerender(
    <VisualizationSession {...props} canDisplay={() => true} />,
  );
  await waitFor(() =>
    expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(2),
  );
  expect(calls.filter((call) => call.name === "viz.view.open")).toHaveLength(1);
  expect(
    calls.filter((call) => call.name === "viz.render").map((call) => call.body),
  ).toMatchObject([
    { requestSeq: 1, depthLimit: 1, viewChannel: "stable-view" },
    { requestSeq: 2, depthLimit: 7, viewChannel: "stable-view" },
  ]);
});

test("a stalled cancel cannot prevent closing the channel on unmount", async () => {
  const calls: CallEnvelope[] = [];
  const never = deferred<never>();
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    const fixture = modelFixture();
    const body =
      envelope.name === "viz.view.open"
        ? { viewChannel: "closing-view" }
        : envelope.name === "viz.render"
          ? {
              kind: "job",
              requestSeq: 1,
              resultRevision: fixture.frame.resultRevision,
              observedAt: null,
              completeness: "partial",
              jobRef: "job-one",
            }
          : envelope.name === "viz.cancel"
            ? await never.promise
            : {};
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    };
  });
  const rendered = render(<VisualizationSession {...props} />);
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.render")).toBe(true),
  );
  rendered.unmount();
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.view.close")).toBe(true),
  );
  expect(calls.filter((call) => call.name === "viz.cancel")).toHaveLength(1);
  expect(calls.find((call) => call.name === "viz.cancel")!.body).toMatchObject({
    target: {
      kind: "subscription",
      viewChannel: "closing-view",
      requestSeq: 1,
    },
  });
  expect(calls.find((call) => call.name === "viz.view.close")!.body).toEqual({
    viewChannel: "closing-view",
  });
});

test("unmount detaches job observation and ignores a late job.get response", async () => {
  const calls: CallEnvelope[] = [];
  const status = deferred<unknown>();
  const listeners = new Set<(event: EventEnvelope) => void>();
  const { props, client } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    const fixture = modelFixture();
    const body =
      envelope.name === "viz.view.open"
        ? { viewChannel: "job-view" }
        : envelope.name === "viz.render"
          ? {
              kind: "job",
              requestSeq: 1,
              resultRevision: fixture.frame.resultRevision,
              observedAt: null,
              completeness: "partial",
              jobRef: "job-late",
            }
          : envelope.name === "job.get"
            ? await status.promise
            : envelope.name === "viz.cancel"
              ? { controlResult: "accepted" }
              : {};
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    };
  });
  client.subscribe = (listener) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  };
  const rendered = render(<VisualizationSession {...props} />);
  await waitFor(() =>
    expect(calls.some((call) => call.name === "job.get")).toBe(true),
  );
  expect(listeners.size).toBe(1);
  rendered.unmount();
  expect(listeners.size).toBe(0);
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.view.close")).toBe(true),
  );
  await act(async () => {
    status.resolve({
      jobRef: "job-late",
      kind: "viz.compute",
      state: "done",
      sequence: 1,
      resultVersion: null,
      reasonKey: null,
    });
  });
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(1);
});

test("late first-page rows after unmount cannot trigger another view operation", async () => {
  const calls: CallEnvelope[] = [];
  const rows = deferred<unknown>();
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    const fixture = modelFixture();
    const body =
      envelope.name === "viz.view.open"
        ? { viewChannel: "rows-view" }
        : envelope.name === "viz.rows"
          ? await rows.promise
          : envelope.name === "viz.render"
            ? {
                ...fixture.frame,
                requestSeq: (envelope.body as { requestSeq: number })
                  .requestSeq,
              }
            : {};
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    };
  });
  const rendered = render(<VisualizationSession {...props} autoLoadRows />);
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.rows")).toBe(true),
  );
  const rowRequest = calls.find((call) => call.name === "viz.rows")!.body as {
    modelRef: string;
    resultRevision: string;
  };
  expect(rowRequest).toMatchObject({
    modelRef: "synthetic-layout",
    resultRevision: request().resultRevision,
  });
  rendered.unmount();
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.view.close")).toBe(true),
  );
  await act(async () => rows.resolve(modelFixture().page.response));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(1);
  expect(calls.filter((call) => call.name === "viz.view.close")).toHaveLength(
    1,
  );
});

test("session carries one pinned model through typed rows and group rows", async () => {
  const calls: CallEnvelope[] = [];
  const fixture = modelFixture();
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    const body =
      envelope.name === "viz.view.open"
        ? { viewChannel: "group-view" }
        : envelope.name === "viz.render"
          ? {
              ...fixture.frame,
              requestSeq: (envelope.body as { requestSeq: number }).requestSeq,
            }
          : envelope.name === "viz.rows"
            ? groupMemberPage()
            : envelope.name === "viz.group_rows"
              ? groupRowsResponse()
              : {};
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    };
  });
  render(
    <VisualizationSession {...props} canDisplay={() => false} autoLoadRows />,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Items in this result" }),
  );
  const groupAction = await screen.findByRole("button", {
    name: /View members of Group A/,
  });
  const renderCall = calls.find((call) => call.name === "viz.render")!;
  const rowsCall = calls.find((call) => call.name === "viz.rows")!;
  expect((renderCall.body as { resultRevision: string }).resultRevision).toBe(
    fixture.frame.resultRevision,
  );
  expect((rowsCall.body as { resultRevision: string }).resultRevision).toBe(
    fixture.frame.resultRevision,
  );
  fireEvent.click(groupAction);
  await screen.findByRole("button", { name: "Back to visualization" });
  const groupCall = calls.find((call) => call.name === "viz.group_rows")!;
  expect(groupCall.body).toMatchObject({
    modelRef: fixture.frame.model.modelRef,
    groupRef: "group-a",
    start: 0,
    count: 100,
  });
  await screen.findByText("Member file");
});

test("group controller drops a late typed group_rows response after unmount", async () => {
  const calls: CallEnvelope[] = [];
  const groupRows = deferred<unknown>();
  const fixture = modelFixture();
  const { props } = setup(async (_, { envelope }) => {
    calls.push(envelope);
    const body =
      envelope.name === "viz.view.open"
        ? { viewChannel: "late-group" }
        : envelope.name === "viz.render"
          ? {
              ...fixture.frame,
              requestSeq: (envelope.body as { requestSeq: number }).requestSeq,
            }
          : envelope.name === "viz.rows"
            ? groupMemberPage()
            : envelope.name === "viz.group_rows"
              ? await groupRows.promise
              : {};
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    };
  });
  const rendered = render(
    <VisualizationSession {...props} canDisplay={() => false} autoLoadRows />,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Items in this result" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /View members of Group A/ }),
  );
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.group_rows")).toBe(true),
  );
  rendered.unmount();
  await waitFor(() =>
    expect(calls.some((call) => call.name === "viz.view.close")).toBe(true),
  );
  await act(async () => groupRows.resolve(groupRowsResponse()));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(calls.filter((call) => call.name === "viz.group_rows")).toHaveLength(
    1,
  );
  expect(calls.filter((call) => call.name === "viz.view.close")).toHaveLength(
    1,
  );
});

test("result or client replacement closes the prior channel and opens a fresh one", async () => {
  const callsA: CallEnvelope[] = [];
  const callsB: CallEnvelope[] = [];
  const makeTransport =
    (calls: CallEnvelope[], channel: string) =>
    async (_: string, { envelope }: { envelope: CallEnvelope }) => {
      calls.push(envelope);
      const fixture = modelFixture();
      const body =
        envelope.name === "viz.view.open"
          ? { viewChannel: channel }
          : envelope.name === "viz.render"
            ? {
                ...fixture.frame,
                requestSeq: (envelope.body as { requestSeq: number })
                  .requestSeq,
              }
            : {};
      return {
        name: envelope.name,
        correlationId: envelope.correlationId,
        contractVersion: envelope.contractVersion,
        body,
      };
    };
  const first = setup(makeTransport(callsA, "client-a"));
  const rendered = render(<VisualizationSession {...first.props} />);
  await waitFor(() =>
    expect(callsA.some((call) => call.name === "viz.render")).toBe(true),
  );
  const second = setup(makeTransport(callsB, "client-b"), request(9));
  rendered.rerender(<VisualizationSession {...second.props} />);
  await waitFor(() =>
    expect(callsB.some((call) => call.name === "viz.render")).toBe(true),
  );
  await waitFor(() =>
    expect(callsA.some((call) => call.name === "viz.view.close")).toBe(true),
  );
  expect(callsA.filter((call) => call.name === "viz.render")).toHaveLength(1);
  expect(callsB.filter((call) => call.name === "viz.view.open")).toHaveLength(
    1,
  );
  expect(
    callsB.filter((call) => call.name === "viz.render")[0]!.body,
  ).toMatchObject({ depthLimit: 9 });
});

test("same-client result replacement cancels and closes the old channel before ignoring its late render", async () => {
  const calls: CallEnvelope[] = [];
  const oldRender = deferred<unknown>();
  const firstRevision = modelFixture().frame.resultRevision;
  const nextRevision = "018f0000-0000-7000-8000-000000000002";
  let openCount = 0;
  const currentClient = setup(async (_, { envelope }) => {
    calls.push(envelope);
    if (envelope.name === "viz.view.open") {
      openCount += 1;
      return {
        name: envelope.name,
        correlationId: envelope.correlationId,
        contractVersion: envelope.contractVersion,
        body: { viewChannel: `revision-view-${openCount}` },
      };
    }
    if (
      envelope.name === "viz.render" &&
      (envelope.body as { resultRevision: string }).resultRevision ===
        firstRevision
    ) {
      return {
        name: envelope.name,
        correlationId: envelope.correlationId,
        contractVersion: envelope.contractVersion,
        body: await oldRender.promise,
      };
    }
    const renderRequest = envelope.body as {
      requestSeq: number;
      resultRevision: string;
    };
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body:
        envelope.name === "viz.render"
          ? {
              ...frameForRevision(renderRequest.resultRevision),
              requestSeq: renderRequest.requestSeq,
            }
          : envelope.name === "viz.cancel"
            ? { controlResult: "accepted" }
            : envelope.name === "viz.view.close"
              ? {}
              : {},
    };
  });
  const rendered = render(<VisualizationSession {...currentClient.props} />);
  await waitFor(() =>
    expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(1),
  );
  rendered.rerender(
    <VisualizationSession
      {...currentClient.props}
      request={requestForRevision(nextRevision)}
    />,
  );
  await waitFor(() =>
    expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(2),
  );
  await waitFor(() =>
    expect(calls.filter((call) => call.name === "viz.view.open")).toHaveLength(
      2,
    ),
  );

  const firstChannel = "revision-view-1";
  const cancels = calls.filter((call) => call.name === "viz.cancel");
  expect(cancels).toHaveLength(1);
  expect(cancels[0]!.body).toMatchObject({
    target: { kind: "subscription", viewChannel: firstChannel, requestSeq: 1 },
  });
  expect(calls.find((call) => call.name === "viz.view.close")!.body).toEqual({
    viewChannel: firstChannel,
  });
  const renderCalls = calls.filter((call) => call.name === "viz.render");
  expect(renderCalls.map((call) => call.body)).toMatchObject([
    { viewChannel: firstChannel, requestSeq: 1, resultRevision: firstRevision },
    {
      viewChannel: "revision-view-2",
      requestSeq: 1,
      resultRevision: nextRevision,
    },
  ]);
  await act(async () => {
    oldRender.resolve({ ...frameForRevision(firstRevision), requestSeq: 1 });
  });
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(2);
  expect(calls.filter((call) => call.name === "viz.view.open")).toHaveLength(2);
});
