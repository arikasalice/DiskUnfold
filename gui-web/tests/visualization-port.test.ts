import { expect, test, vi } from "vitest";
import { createAnalysisClient } from "../src/ipc";
import type {
  CallEnvelope,
  VizGroupRowsRequest,
  VizRenderRequest,
} from "../src/ipc";
import {
  createVisualizationPort,
  observeVisualizationJob,
} from "../src/features/visualization/visualization-port";
import { VisualizationController } from "../src/features/visualization/controller";
import { modelFixture } from "../preview/model-fixture";

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
  const request: VizRenderRequest = {
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
  const request: VizRenderRequest = {
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
