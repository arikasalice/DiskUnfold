import { expect, test } from "vitest";
import { AnalysisError, createAnalysisClient } from "../src/ipc";
import type {
  CallEnvelope,
  ResultEnvelope,
  VizLocate,
  VizRows,
  VizRowsRequest,
} from "../src/ipc";

const revision = "0195f856-ab83-7a59-9343-10d29775c9a9";
const otherRevision = "0195f856-ab83-7a59-9343-10d29775c9aa";
const rows: VizRows = {
  rows: [],
  totalCount: 0,
  resultRevision: revision,
  observedAt: null,
  completeness: "complete",
};
const request: VizRowsRequest = {
  modelRef: "model-a",
  resultRevision: revision,
  start: 0,
  count: 25,
};
function response(envelope: CallEnvelope, body: unknown): ResultEnvelope {
  return {
    name: envelope.name,
    contractVersion: 1,
    correlationId: envelope.correlationId,
    body,
  };
}

test("viz.rows sends the pinned model and result version without changing values", async () => {
  let sent: CallEnvelope | undefined;
  const client = createAnalysisClient(async (_, { envelope }) => {
    sent = envelope;
    return response(envelope, rows);
  });
  await expect(client.call("viz.rows", request)).resolves.toEqual(rows);
  expect(sent?.name).toBe("viz.rows");
  expect(sent?.body).toEqual(request);
});

test("concurrent delayed rows responses keep each request's result binding", async () => {
  const pending: Array<{
    envelope: CallEnvelope;
    resolve: (result: ResultEnvelope) => void;
  }> = [];
  const client = createAnalysisClient(
    (_, { envelope }) =>
      new Promise((resolve) => pending.push({ envelope, resolve })),
  );
  const first = client.call("viz.rows", request);
  const second = client.call("viz.rows", {
    ...request,
    modelRef: "model-b",
    resultRevision: otherRevision,
  });
  const [a, b] = pending;
  if (!a || !b) throw new Error("missing transport requests");
  b.resolve(response(b.envelope, { ...rows, resultRevision: otherRevision }));
  a.resolve(response(a.envelope, { ...rows, resultRevision: otherRevision }));
  await expect(second).resolves.toMatchObject({
    resultRevision: otherRevision,
  });
  await expect(first).rejects.toThrow("mismatched result revision");
});

test("viz.rows preserves server errors as AnalysisError without retry", async () => {
  let calls = 0;
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls += 1;
    return {
      ...response(envelope, null),
      error: {
        code: "STALE_VERSION",
        reasonKey: "result.requery_required",
        messageKey: "result.requery_required",
        correlationId: envelope.correlationId,
        retryable: false,
      },
    };
  });
  await expect(client.call("viz.rows", request)).rejects.toMatchObject(
    new AnalysisError("STALE_VERSION", "result.requery_required"),
  );
  expect(calls).toBe(1);
});

test("viz.locate sends the model and item reference and returns the typed location", async () => {
  const location: VizLocate = { location: "group", groupRef: "group-a" };
  const body = {
    modelRef: "model-a",
    itemRef: { resultVersion: revision, itemNo: 3 },
  };
  const client = createAnalysisClient(async (_, { envelope }) => {
    expect(envelope.name).toBe("viz.locate");
    expect(envelope.body).toEqual(body);
    return response(envelope, location);
  });
  await expect(client.call("viz.locate", body)).resolves.toEqual(location);
});

test("a pending rows call keeps its original revision if the caller changes its object", async () => {
  const body = { ...request };
  let reply: ((result: ResultEnvelope) => void) | undefined;
  let sent: CallEnvelope | undefined;
  const client = createAnalysisClient((_, { envelope }) => {
    sent = envelope;
    return new Promise((resolve) => {
      reply = resolve;
    });
  });
  const pending = client.call("viz.rows", body);
  body.resultRevision = otherRevision;
  if (!sent || !reply) throw new Error("missing transport request");
  expect(sent.body).toEqual(request);
  reply(response(sent, rows));
  await expect(pending).resolves.toEqual(rows);
});
