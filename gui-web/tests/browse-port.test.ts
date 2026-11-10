import { expect, test } from "vitest";
import { createBrowsePort } from "../src/app/browse-port";
import { createAnalysisClient } from "../src/ipc";
import type { BrowseOpenRequest, CallEnvelope } from "../src/ipc";

const request = {
  viewKind: "list",
  resultVersion: "pinned-version",
  requestSeq: 3,
  pageSize: 200,
} satisfies Omit<BrowseOpenRequest, "regionRef">;

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
    if (envelope.name === "query.region.open")
      return reply(envelope, { regionRef: "region" });
    switch (envelope.name) {
      case "browse.open":
        expect(envelope.body).toEqual({ ...request, regionRef: "region" });
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
  expect(calls).toEqual([
    "query.region.open",
    "browse.open",
    "job.get",
    "query.page",
  ]);
});

test("a completed page without location context fails without reopening", async () => {
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    if (envelope.name === "query.region.open")
      return reply(envelope, { regionRef: "region" });
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
  expect(calls).toEqual([
    "query.region.open",
    "browse.open",
    "job.get",
    "query.page",
  ]);
});

test("leaving a pending folder stops polling and cancels only its job", async () => {
  const controller = new AbortController();
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    if (envelope.name === "query.region.open")
      return reply(envelope, { regionRef: "region" });
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
      case "query.cancel":
        expect(envelope.body).toEqual({
          regionRef: "region",
          target: { kind: "job", jobRef: "pending-job" },
        });
        return reply(envelope, { outcome: "accepted" });
      default:
        throw new Error(`Unexpected command ${envelope.name}`);
    }
  });
  await expect(
    createBrowsePort(client).open(request, controller.signal),
  ).rejects.toMatchObject({ name: "AbortError" });
  expect(calls).toEqual([
    "query.region.open",
    "browse.open",
    "job.get",
    "query.cancel",
  ]);
});

test("an aborted folder never dispatches an open or returns a late immediate page", async () => {
  const calls: string[] = [];
  const controller = new AbortController();
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    if (envelope.name === "query.region.open")
      return reply(envelope, { regionRef: "region" });
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
  expect(calls).toEqual([
    "query.region.open",
    "browse.open",
    "query.cancel",
    "query.region.close",
  ]);
});

test("a finished job from a superseded folder does not read a page or reopen", async () => {
  const controller = new AbortController();
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    if (envelope.name === "query.region.open")
      return reply(envelope, { regionRef: "region" });
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
      case "query.cancel":
        expect(envelope.body).toEqual({
          regionRef: "region",
          target: { kind: "job", jobRef: "superseded-job" },
        });
        return reply(envelope, { outcome: "already_terminal" });
      default:
        throw new Error(`Unexpected command ${envelope.name}`);
    }
  });

  await expect(
    createBrowsePort(client).open(request, controller.signal),
  ).rejects.toMatchObject({ name: "AbortError" });
  expect(calls).toEqual([
    "query.region.open",
    "browse.open",
    "job.get",
    "query.cancel",
  ]);
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
    if (envelope.name === "query.region.open")
      return reply(envelope, { regionRef: "region" });
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
    if (envelope.name === "query.cancel") {
      expect(envelope.body).toEqual({
        regionRef: "region",
        target: { kind: "job", jobRef: "old-job" },
      });
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
    "query.region.open",
    "browse.open",
    "job.get",
    "query.page",
    "browse.open",
    "query.cancel",
  ]);
});

test("disposing before region issuance closes the late region once", async () => {
  let release: ((value: ReturnType<typeof reply>) => void) | undefined;
  let opened: (() => void) | undefined;
  const started = new Promise<void>((resolve) => {
    opened = resolve;
  });
  const calls: string[] = [];
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    if (envelope.name === "query.region.open") {
      opened?.();
      return new Promise((resolve) => {
        release = resolve;
      });
    }
    if (envelope.name === "query.region.close") {
      expect(envelope.body).toEqual({ regionRef: "late-region" });
      return reply(envelope, {});
    }
    throw new Error(`Unexpected command ${envelope.name}`);
  });
  const port = createBrowsePort(client);
  const pending = port.open(request);
  await started;
  port.dispose();
  port.dispose();
  release?.({
    name: "query.region.open",
    correlationId: "unused",
    contractVersion: 1,
    body: { regionRef: "late-region" },
  });
  await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  await Promise.resolve();
  expect(calls).toEqual(["query.region.open", "query.region.close"]);
});

test("sequence exhaustion retires the region and resumes at one", async () => {
  const calls: string[] = [];
  let issued = 0;
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push(envelope.name);
    if (envelope.name === "query.region.open")
      return reply(envelope, { regionRef: `region-${++issued}` });
    if (envelope.name === "query.region.close") return reply(envelope, {});
    if (envelope.name === "browse.open") {
      const body = envelope.body as BrowseOpenRequest;
      expect(body.requestSeq).toBe(issued === 1 ? 0xffff_ffff : 1);
      return reply(envelope, {
        kind: "opened",
        requestSeq: body.requestSeq,
        page,
      });
    }
    throw new Error(`Unexpected command ${envelope.name}`);
  });
  const port = createBrowsePort(client);
  await expect(
    port.open({ ...request, requestSeq: 0xffff_ffff }),
  ).resolves.toMatchObject({ requestSeq: 0xffff_ffff });
  await expect(
    port.open({ ...request, requestSeq: 0x1_0000_0000 }),
  ).resolves.toMatchObject({ requestSeq: 0x1_0000_0000 });
  expect(calls).toEqual([
    "query.region.open",
    "browse.open",
    "query.region.open",
    "query.region.close",
    "browse.open",
  ]);
});

test("a job received after region close is cancelled by its exact reference", async () => {
  const calls: Array<{ name: string; body: unknown }> = [];
  let release: ((value: ReturnType<typeof reply>) => void) | undefined;
  let markStarted: (() => void) | undefined;
  const started = new Promise<void>((resolve) => {
    markStarted = resolve;
  });
  const controller = new AbortController();
  const client = createAnalysisClient(async (_, { envelope }) => {
    calls.push({ name: envelope.name, body: envelope.body });
    if (envelope.name === "query.region.open")
      return reply(envelope, { regionRef: "region" });
    if (envelope.name === "browse.open") {
      markStarted?.();
      return new Promise((resolve) => {
        release = resolve;
      });
    }
    if (
      ["query.cancel", "query.region.close", "job.cancel"].includes(
        envelope.name,
      )
    )
      return reply(envelope, {});
    throw new Error(`Unexpected command ${envelope.name}`);
  });
  const port = createBrowsePort(client);
  const pending = port.open(request, controller.signal);
  await started;
  controller.abort();
  release?.({
    name: "browse.open",
    correlationId: "unused",
    contractVersion: 1,
    body: { kind: "job", requestSeq: 3, jobRef: "late-job" },
  });
  await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  expect(calls.filter((call) => call.name === "job.cancel")).toEqual([
    { name: "job.cancel", body: { jobRef: "late-job" } },
  ]);
});

test.each([
  ["job.get", "queued"],
  ["job.get", "done"],
  ["query.page", "done"],
] as const)(
  "dispose fences a delayed %s response in %s state without a signal",
  async (delayedCommand, state) => {
    let release: (() => void) | undefined;
    let markStarted: (() => void) | undefined;
    const started = new Promise<void>((resolve) => {
      markStarted = resolve;
    });
    const waiting = new Promise<void>((resolve) => {
      release = resolve;
    });
    const calls: Array<{ name: string; body: unknown }> = [];
    const client = createAnalysisClient(async (_, { envelope }) => {
      calls.push({ name: envelope.name, body: envelope.body });
      if (envelope.name === "query.region.open")
        return reply(envelope, { regionRef: "region" });
      if (envelope.name === "browse.open")
        return reply(envelope, {
          kind: "job",
          requestSeq: 3,
          jobRef: "target-job",
        });
      if (envelope.name === delayedCommand) {
        markStarted?.();
        await waiting;
      }
      if (envelope.name === "job.get")
        return reply(envelope, {
          jobRef: "target-job",
          kind: "query.evaluate",
          state,
          ...(state === "done" ? { resultVersion: page.queryRef } : {}),
        });
      if (envelope.name === "query.page") return reply(envelope, page);
      if (
        envelope.name === "query.region.close" ||
        envelope.name === "job.cancel"
      )
        return reply(envelope, {});
      throw new Error(`Unexpected command ${envelope.name}`);
    });
    const port = createBrowsePort(client);
    const pending = port.open(request);
    await started;
    port.dispose();
    release?.();
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(calls.filter(({ name }) => name === "job.cancel")).toEqual([
      { name: "job.cancel", body: { jobRef: "target-job" } },
    ]);
    expect(calls.filter(({ name }) => name === "job.get")).toHaveLength(1);
    expect(calls.filter(({ name }) => name === "query.page")).toHaveLength(
      delayedCommand === "query.page" ? 1 : 0,
    );
    await expect(port.open(request)).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(
      calls.filter(({ name }) => name === "query.region.open"),
    ).toHaveLength(1);
  },
);
