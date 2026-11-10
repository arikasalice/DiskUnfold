import { expect, test } from "vitest";
import { createBrowsePort } from "../src/app/browse-port";
import { createAnalysisClient } from "../src/ipc";
import type { BrowseOpenRequest, CallEnvelope } from "../src/ipc";

const request = {
  viewKind: "list",
  resultVersion: "pinned-version",
  requestSeq: 3,
  pageSize: 200,
} satisfies BrowseOpenRequest;

const page = {
  rowKind: "folder",
  queryRef: "published-query",
  resultVersion: request.resultVersion,
  rows: [],
  total: { state: "exact", count: 0 },
  unknownCount: 0,
  completeness: "complete",
  nextCursor: null,
  observedAt: null,
};

function reply(envelope: CallEnvelope, body: unknown) {
  return {
    name: envelope.name,
    correlationId: envelope.correlationId,
    contractVersion: envelope.contractVersion,
    body,
  };
}

test("a finished folder job fetches its pinned page before reopening cached breadcrumbs", async () => {
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    switch (envelope.name) {
      case "browse.open":
        expect(envelope.body).toEqual(request);
        return reply(
          envelope,
          calls.length === 1
            ? { kind: "job", requestSeq: 3, jobRef: "folder-job" }
            : { kind: "opened", requestSeq: 3, breadcrumbs: [], page },
        );
      case "job.get":
        expect(envelope.body).toEqual({ jobRef: "folder-job" });
        return reply(envelope, {
          jobRef: "folder-job",
          kind: "query.evaluate",
          state: "done",
          resultVersion: page.queryRef,
        });
      case "query.page":
        expect(envelope.body).toEqual({
          queryRef: page.queryRef,
          resultVersion: request.resultVersion,
          start: 0,
          count: 200,
        });
        return reply(envelope, page);
      default:
        throw new Error(`Unexpected command ${envelope.name}`);
    }
  });
  await expect(createBrowsePort(client).open(request)).resolves.toMatchObject({
    kind: "opened",
    page,
  });
  expect(calls).toEqual([
    "browse.open",
    "job.get",
    "query.page",
    "browse.open",
  ]);
});

test("leaving a pending folder stops polling and cancels only its job", async () => {
  const controller = new AbortController();
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    switch (envelope.name) {
      case "browse.open":
        return reply(envelope, {
          kind: "job",
          requestSeq: 3,
          jobRef: "pending-job",
        });
      case "job.get":
        controller.abort();
        return reply(envelope, {
          jobRef: "pending-job",
          kind: "query.evaluate",
          state: "queued",
        });
      case "job.cancel":
        expect(envelope.body).toEqual({ jobRef: "pending-job" });
        return reply(envelope, { outcome: "accepted" });
      default:
        throw new Error(`Unexpected command ${envelope.name}`);
    }
  });
  await expect(
    createBrowsePort(client).open(request, controller.signal),
  ).rejects.toMatchObject({ name: "AbortError" });
  expect(calls).toEqual(["browse.open", "job.get", "job.cancel"]);
});
