import { expect, test, vi } from "vitest";
import { createAnalysisClient } from "../src/ipc";
import type {
  CallEnvelope,
  EventEnvelope,
  VizGroupRowsRequest,
  VizRenderRequest,
} from "../src/ipc";
import {
  createVisualizationPort,
  observeVisualizationJob,
} from "../src/features/visualization/visualization-port";
import { VisualizationController } from "../src/features/visualization/controller";
import { modelFixture } from "../preview/model-fixture";

function fixtureRequest(revision: string): VizRenderRequest {
  return {
    viewChannel: "view",
    requestSeq: 1,
    resultRevision: revision,
    format: "treemap",
    aggregateScopeRef: { resultVersion: revision, itemNo: 0 },
    navigationScopeRef: { resultVersion: revision, itemNo: 0 },
    measure: "size",
    sizeBasis: "logical",
    classificationCriterion: "type",
    colorCriterion: "type",
    denominatorBasis: "scope",
    viewport: { width: 960, height: 330, scale: 1 },
    visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
    minAreaPx2: 4,
    depthLimit: 1,
  };
}

const resyncEvent: EventEnvelope = {
  topic: "job.resync_required",
  contractVersion: 1,
  seq: 1,
  occurredAt: "2026-11-12T00:00:00Z",
  body: null,
};

test("visualization port forwards published commands through the real typed client", async () => {
  const calls: CallEnvelope[] = [];
  const fixture = modelFixture();
  const revision = fixture.frame.resultRevision;
  const groupRequest: VizGroupRowsRequest = {
    modelRef: fixture.frame.model.modelRef,
    groupRef: "group-a",
    start: 0,
    count: 10,
  };
  const groupResponse = {
    ...fixture.page.response,
    groupTotal: fixture.page.response.rows[0]!.size,
    sizeRange: { kind: "unknown" as const },
  };
  const request = fixtureRequest(revision);
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope);
    const body =
      envelope.name === "viz.view.open"
        ? { viewChannel: "view" }
        : envelope.name === "viz.render"
          ? fixture.frame
          : envelope.name === "viz.rows"
            ? fixture.page.response
            : envelope.name === "viz.group_rows"
              ? groupResponse
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
  const port = createVisualizationPort(client, () => true);
  expect(await port.open()).toEqual({ viewChannel: "view" });
  expect(await port.render(request)).toEqual(fixture.frame);
  const rows = {
    modelRef: fixture.frame.model.modelRef,
    resultRevision: revision,
    start: 0,
    count: 20,
  };
  expect(await port.rows(rows)).toEqual(fixture.page.response);
  const groupRows = port.groupRows(groupRequest);
  groupRequest.groupRef = "mutated-after-call";
  expect(await groupRows).toEqual(groupResponse);
  await port.cancel({
    target: { kind: "subscription", viewChannel: "view", requestSeq: 1 },
  });
  await port.close("view");
  expect(calls.map(({ name, body }) => ({ name, body }))).toEqual([
    { name: "viz.view.open", body: {} },
    { name: "viz.render", body: request },
    { name: "viz.rows", body: rows },
    { name: "viz.group_rows", body: { ...groupRequest, groupRef: "group-a" } },
    {
      name: "viz.cancel",
      body: {
        target: { kind: "subscription", viewChannel: "view", requestSeq: 1 },
      },
    },
    { name: "viz.view.close", body: { viewChannel: "view" } },
  ]);
});

test("host failures propagate instead of returning a fabricated model", async () => {
  const client = createAnalysisClient(async (_, { envelope }) => ({
    name: envelope.name,
    correlationId: envelope.correlationId,
    contractVersion: envelope.contractVersion,
    body: null,
    error: {
      code: "NOT_FOUND",
      reasonKey: "ref.unavailable",
      messageKey: "ref.unavailable",
      correlationId: envelope.correlationId,
      retryable: false,
    },
  }));
  await expect(
    createVisualizationPort(client, () => true).groupRows({
      modelRef: "model",
      groupRef: "group",
      start: 0,
      count: 10,
    }),
  ).rejects.toMatchObject({ code: "NOT_FOUND" });
});

test("job observer uses typed job.get and re-queries the same pinned request", async () => {
  const fixture = modelFixture();
  const revision = fixture.frame.resultRevision;
  const request = fixtureRequest(revision);
  const calls: CallEnvelope[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope);
    const body =
      envelope.name === "job.get"
        ? {
            jobRef: "job",
            kind: "viz.compute",
            state: "done",
            sequence: 2,
            resultVersion: null,
            reasonKey: null,
          }
        : (envelope.body as VizRenderRequest).requestSeq === 1
          ? {
              kind: "job",
              requestSeq: 1,
              resultRevision: revision,
              observedAt: null,
              completeness: "partial",
              jobRef: "job",
            }
          : { ...fixture.frame, requestSeq: 2 };
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    };
  });
  client.subscribe = () => () => {};
  const controller = new VisualizationController(
    createVisualizationPort(client, () => true),
  );
  const stop = observeVisualizationJob(controller, client);
  try {
    await controller.render(request);
    await vi.waitFor(() =>
      expect(controller.getSnapshot().graph?.frame.requestSeq).toBe(2),
    );
    expect(
      calls.filter((call) => call.name === "job.get").map((call) => call.body),
    ).toEqual([{ jobRef: "job" }]);
    expect(
      calls
        .filter((call) => call.name === "viz.render")
        .map((call) => call.body),
    ).toEqual([request, { ...request, requestSeq: 2 }]);
  } finally {
    stop();
  }
});

test("resync lists jobs and checks the exact displayed model without rendering again", async () => {
  const fixture = modelFixture();
  const revision = fixture.frame.resultRevision;
  const calls: CallEnvelope[] = [];
  let stale = false;
  let receive: (event: EventEnvelope) => void = () => {};
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope);
    return {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body:
        envelope.name === "job.list"
          ? []
          : envelope.name === "viz.rows"
            ? fixture.page.response
            : fixture.frame,
      ...(envelope.name === "viz.rows" && stale
        ? {
            error: {
              code: "STALE_VERSION",
              reasonKey: "result.version_stale",
              messageKey: "result.version_stale",
              correlationId: envelope.correlationId,
              retryable: false,
            },
          }
        : {}),
    };
  });
  client.subscribe = (listener) => {
    receive = listener;
    return () => {};
  };
  const controller = new VisualizationController(
    createVisualizationPort(client, () => true),
  );
  const stop = observeVisualizationJob(controller, client);
  try {
    await controller.render(fixtureRequest(revision));
    const visible = controller.getSnapshot().graph;
    expect(visible).not.toBeNull();
    stale = true;
    receive(resyncEvent);
    await vi.waitFor(() =>
      expect(controller.getSnapshot().revisionCheck).toBe("stale"),
    );
    expect(controller.getSnapshot().graph).toBe(visible);
    expect(controller.getSnapshot().tablePage).toBeNull();
    expect(calls.map((call) => call.name)).toEqual([
      "viz.render",
      "job.list",
      "viz.rows",
    ]);
    expect(calls[2]?.body).toEqual({
      modelRef: fixture.frame.model.modelRef,
      resultRevision: revision,
      start: 0,
      count: 1,
    });
    stale = false;
    receive(resyncEvent);
    await vi.waitFor(() =>
      expect(controller.getSnapshot().revisionCheck).toBeNull(),
    );
    expect(controller.getSnapshot().graph).toBe(visible);
    expect(calls.filter((call) => call.name === "viz.render")).toHaveLength(1);
  } finally {
    stop();
  }
});

test("a resync list cut fences job status below its sequence", async () => {
  const fixture = modelFixture();
  const revision = fixture.frame.resultRevision;
  let receive: (event: EventEnvelope) => void = () => {};
  let firstGet = true;
  let listed = false;
  let sequence = 4;
  let release: () => void = () => {};
  const blockedGet = new Promise<void>((resolve) => {
    release = resolve;
  });
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    const reply = (body: unknown) => ({
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: envelope.contractVersion,
      body,
    });
    if (envelope.name === "job.list") {
      listed = true;
      return reply([{ jobRef: "job", sequence: 5 }]);
    }
    if (envelope.name === "job.get") {
      const observed = sequence;
      if (firstGet) {
        firstGet = false;
        await blockedGet;
      }
      return reply({
        jobRef: "job",
        kind: "viz.compute",
        state: "done",
        sequence: observed,
        resultVersion: null,
        reasonKey: null,
      });
    }
    return reply(
      (envelope.body as VizRenderRequest).requestSeq === 1
        ? {
            kind: "job",
            requestSeq: 1,
            resultRevision: revision,
            observedAt: null,
            completeness: "partial",
            jobRef: "job",
          }
        : { ...fixture.frame, requestSeq: 2 },
    );
  });
  client.subscribe = (listener) => {
    receive = listener;
    return () => {};
  };
  const controller = new VisualizationController(
    createVisualizationPort(client, () => true),
  );
  const stop = observeVisualizationJob(controller, client);
  try {
    await controller.render(fixtureRequest(revision));
    expect(firstGet).toBe(false);
    receive(resyncEvent);
    expect(listed).toBe(true);
    release();
    await vi.waitFor(() =>
      expect(
        calls.filter((name) => name === "job.get").length,
      ).toBeGreaterThanOrEqual(2),
    );
    expect(controller.getSnapshot().phase).toBe("job");
    expect(calls.filter((name) => name === "viz.render")).toHaveLength(1);
    sequence = 5;
    receive({ ...resyncEvent, topic: "job.finished", jobRef: "job" });
    await vi.waitFor(() =>
      expect(controller.getSnapshot().graph?.frame.requestSeq).toBe(2),
    );
  } finally {
    release();
    stop();
  }
});
