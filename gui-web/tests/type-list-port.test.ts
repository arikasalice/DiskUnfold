import { expect, test } from "vitest";
import { createTypeListPort } from "../src/app/type-list-port";
import { createAnalysisClient } from "../src/ipc";
import type { CallEnvelope, QueryCondition } from "../src/ipc";

const version = "pinned-version";
const scope = { resultVersion: version, itemNo: 4 };
const condition: QueryCondition = { kind: "all", children: [] };
const page = {
  rowKind: "search",
  queryRef: "completed-query",
  resultVersion: version,
  rows: [],
  total: { state: "exact", count: 0 },
  unknownCount: 0,
  observedAt: null,
  completeness: "complete",
  nextCursor: null,
};

function reply(envelope: CallEnvelope, body: unknown) {
  return {
    name: envelope.name,
    correlationId: envelope.correlationId,
    contractVersion: envelope.contractVersion,
    body,
  };
}

test("#306: a list that outlives the engine's wait is read from its completed job", async () => {
  const calls: { name: string; body: unknown }[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push({ name: envelope.name, body: envelope.body });
    switch (envelope.name) {
      case "query.region.open":
        return reply(envelope, { regionRef: "region" });
      case "browse.detail":
        return reply(envelope, {
          item: scope,
          displayPath: { state: "present", value: "D:\\scope" },
        });
      case "query.run":
        return reply(envelope, {
          kind: "job",
          requestSeq: 1,
          jobRef: "job",
          resultVersion: version,
        });
      case "job.get":
        return reply(envelope, {
          jobRef: "job",
          kind: "query.evaluate",
          state: "done",
          queryRef: "completed-query",
        });
      case "query.page":
        return reply(envelope, page);
      default:
        throw new Error(`Unexpected command ${envelope.name}`);
    }
  });
  const opened = await createTypeListPort(client).open(
    { resultVersion: version, scope, condition, sortKeys: [] },
    new AbortController().signal,
  );
  expect(opened.page.queryRef).toBe("completed-query");
  expect(opened.scopePath).toBe("D:\\scope");
  expect(calls.find((call) => call.name === "query.region.open")?.body).toEqual(
    { kind: "search" },
  );
  expect(calls.find((call) => call.name === "query.run")?.body).toMatchObject({
    regionRef: "region",
    requestSeq: 1,
    scope,
    pageSize: 200,
  });
  expect(calls.find((call) => call.name === "query.page")?.body).toEqual({
    queryRef: "completed-query",
    resultVersion: version,
    start: 0,
    count: 200,
  });
});
