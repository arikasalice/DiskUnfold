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
  browseContext: {
    origin: "scan",
    location: null,
    breadcrumbs: [],
    rootKind: "virtual_root",
    currentCorrespondence: false,
    sortStrategies: [],
    predicateStrategies: [],
  },
};

function reply(envelope: CallEnvelope, body: unknown) {
  return {
    name: envelope.name,
    correlationId: envelope.correlationId,
    contractVersion: envelope.contractVersion,
    body,
  };
}

test("a finished folder job uses context from its pinned page", async () => {
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    switch (envelope.name) {
      case "browse.open":
        expect(envelope.body).toEqual(request);
        return reply(envelope, {
          kind: "job",
          requestSeq: 3,
          jobRef: "folder-job",
        });
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
  const result = await createBrowsePort(client).open(request);
  expect(result).toMatchObject({
    kind: "opened",
    page: { queryRef: page.queryRef },
  });
  if (result.kind !== "opened") throw new Error("Expected opened location");
  expect(result.page).not.toHaveProperty("browseContext");
  expect(calls).toEqual(["browse.open", "job.get", "query.page"]);
});

test("a completed page without location context fails without reopening", async () => {
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    if (envelope.name === "browse.open")
      return reply(envelope, {
        kind: "job",
        requestSeq: 3,
        jobRef: "folder-job",
      });
    if (envelope.name === "job.get")
      return reply(envelope, {
        jobRef: "folder-job",
        kind: "query.evaluate",
        state: "done",
        resultVersion: page.queryRef,
      });
    if (envelope.name === "query.page")
      return reply(envelope, { ...page, browseContext: undefined });
    throw new Error(`Unexpected command ${envelope.name}`);
  });
  await expect(createBrowsePort(client).open(request)).rejects.toThrow(
    "no location context",
  );
  expect(calls).toEqual(["browse.open", "job.get", "query.page"]);
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

test("an aborted folder never dispatches an open or returns a late immediate page", async () => {
  const calls: string[] = [];
  const controller = new AbortController();
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    controller.abort();
    return reply(envelope, { kind: "opened", requestSeq: 3, page });
  });
  const port = createBrowsePort(client);

  await expect(port.open(request, AbortSignal.abort())).rejects.toMatchObject({
    name: "AbortError",
  });
  expect(calls).toEqual([]);
  await expect(port.open(request, controller.signal)).rejects.toMatchObject({
    name: "AbortError",
  });
  expect(calls).toEqual(["browse.open"]);
});

test("a finished job from a superseded folder does not read a page or reopen", async () => {
  const controller = new AbortController();
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    switch (envelope.name) {
      case "browse.open":
        return reply(envelope, {
          kind: "job",
          requestSeq: 3,
          jobRef: "superseded-job",
        });
      case "job.get":
        controller.abort();
        return reply(envelope, {
          jobRef: "superseded-job",
          kind: "query.evaluate",
          state: "done",
          resultVersion: page.queryRef,
        });
      case "query.page":
        return reply(envelope, page);
      case "job.cancel":
        expect(envelope.body).toEqual({ jobRef: "superseded-job" });
        return reply(envelope, { outcome: "already_terminal" });
      default:
        throw new Error(`Unexpected command ${envelope.name}`);
    }
  });

  await expect(
    createBrowsePort(client).open(request, controller.signal),
  ).rejects.toMatchObject({ name: "AbortError" });
  expect(calls).toEqual(["browse.open", "job.get", "job.cancel"]);
});

test("aborting a completed query during page retrieval leaves a new navigation alone", async () => {
  const controller = new AbortController();
  const calls: string[] = [];
  let releasePage: (() => void) | undefined;
  const waiting = new Promise<void>((resolve) => {
    releasePage = resolve;
  });
  let pageStarted: (() => void) | undefined;
  const started = new Promise<void>((resolve) => {
    pageStarted = resolve;
  });
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    if (envelope.name === "browse.open") {
      if ((envelope.body as BrowseOpenRequest).requestSeq === 4) {
        return reply(envelope, { kind: "opened", requestSeq: 4, page });
      }
      return reply(envelope, { kind: "job", requestSeq: 3, jobRef: "old-job" });
    }
    if (envelope.name === "job.get")
      return reply(envelope, {
        jobRef: "old-job",
        kind: "query.evaluate",
        state: "done",
        resultVersion: page.queryRef,
      });
    if (envelope.name === "query.page") {
      pageStarted?.();
      await waiting;
      return reply(envelope, page);
    }
    if (envelope.name === "job.cancel") {
      expect(envelope.body).toEqual({ jobRef: "old-job" });
      return reply(envelope, { outcome: "already_terminal" });
    }
    throw new Error(`Unexpected command ${envelope.name}`);
  });
  const port = createBrowsePort(client);
  const old = port.open(request, controller.signal);
  await started;
  controller.abort();
  await expect(port.open({ ...request, requestSeq: 4 })).resolves.toMatchObject(
    { kind: "opened", requestSeq: 4 },
  );
  releasePage?.();
  await expect(old).rejects.toMatchObject({ name: "AbortError" });
  expect(calls).toEqual([
    "browse.open",
    "job.get",
    "query.page",
    "browse.open",
    "job.cancel",
  ]);
});
