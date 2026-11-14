import { vi } from "vitest";
import { AnalysisError } from "../src/ipc";
import type {
  AnalysisClient,
  EventEnvelope,
  GroupMemberRow,
  GroupRuleset,
  GroupSummary,
  GroupTotals,
  SizeValue,
} from "../src/ipc";

export const RESULT = "018f0000-0000-7000-8000-000000000001";
export const NEWER = "018f0000-0000-7000-8000-000000000002";
export const CLASSIFICATION = "018f0000-0000-7000-8000-0000000000c1";
export const MOVIES = "018f0000-0000-7000-8000-0000000000a1";
export const ARCHIVE = "018f0000-0000-7000-8000-0000000000a2";
export const SCOPE = { resultVersion: RESULT, itemNo: 4 };

/** Request bodies are inspected loosely: each test knows the command it fakes. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Body = any;
export type Call = { name: string; body: Body };
export type Handler = (body: Body, calls: Call[]) => unknown;

/** An IPC client whose commands are answered by handlers; it records every call. */
export function fakeClient(handlers: Record<string, Handler>) {
  const calls: Call[] = [];
  const listeners = new Set<(event: EventEnvelope) => void>();
  const client = {
    call: vi.fn(async (name: string, body: unknown) => {
      calls.push({ name, body });
      const handler = handlers[name];
      if (!handler)
        throw new AnalysisError("NOT_FOUND", "command.not_provided");
      return handler(body, calls);
    }),
    subscribe: vi.fn((listener: (event: EventEnvelope) => void) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    }),
  } as unknown as AnalysisClient;
  let sequence = 0;
  return {
    client,
    calls,
    names: () => calls.map((call) => call.name),
    count: (name: string) => calls.filter((call) => call.name === name).length,
    emit(topic: string, extra: Partial<EventEnvelope> = {}) {
      sequence += 1;
      const event: EventEnvelope = {
        topic,
        contractVersion: 1,
        seq: sequence,
        occurredAt: "2026-11-14T00:00:00.000Z",
        body: {},
        ...extra,
      };
      for (const listener of [...listeners]) listener(event);
    },
    listenerCount: () => listeners.size,
  };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

export const flush = () => new Promise<void>((done) => setTimeout(done, 0));

export function ruleset(overrides: Partial<GroupRuleset> = {}): GroupRuleset {
  const nameCondition = {
    kind: "predicate" as const,
    predicate: {
      kind: "name" as const,
      op: "contains" as const,
      encoding: "windows_utf16le" as const,
      // "mp4" in UTF-16LE, base64url.
      value: "bQBwADQA",
    },
  };
  return {
    rulesetVersion: 3,
    revision: 7,
    fingerprint: "aa11",
    updatedAt: "2026-11-13T09:00:00.000Z",
    rules: [
      {
        groupId: MOVIES,
        displayName: "Movies",
        displayOrder: 0,
        condition: nameCondition,
        conditionModelVersion: 1,
        predicates: [],
        review: "ok",
      },
      {
        groupId: ARCHIVE,
        displayName: "Archive",
        displayOrder: 1,
        condition: {
          kind: "predicate",
          predicate: { kind: "type", value: "file" },
        },
        conditionModelVersion: 1,
        predicates: [],
        review: "ok",
      },
    ],
    ...overrides,
  };
}

export function summary(overrides: Partial<GroupSummary> = {}): GroupSummary {
  return {
    classificationRef: CLASSIFICATION,
    resultVersion: RESULT,
    sourceKind: "scan",
    observedAt: "2026-11-01T05:32:00.000Z",
    completeness: "complete",
    rulesetVersion: 3,
    rulesetFingerprint: "aa11",
    groups: [
      {
        groupId: MOVIES,
        displayName: "Movies",
        displayOrder: 0,
        memberCount: 3,
        overlapMemberCount: 1,
        undecidableCount: 0,
        ruleUnevaluable: false,
      },
      {
        groupId: ARCHIVE,
        displayName: "Archive",
        displayOrder: 1,
        memberCount: 5,
        overlapMemberCount: 1,
        undecidableCount: 2,
        ruleUnevaluable: false,
      },
    ],
    scopeFileCount: 20,
    matchedCount: 7,
    overlapCount: 1,
    unclassifiedCount: 11,
    undecidableCount: 2,
    reconciled: true,
    undecidableReasons: [],
    limitations: [{ key: "group.limit.partial" }],
    predicates: [],
    ...overrides,
  };
}

export const exact = (bytes: string): SizeValue => ({
  basis: "logical",
  evidence: "measured",
  state: "exact",
  bytes,
});
export const unknownSize: SizeValue = {
  basis: "logical",
  evidence: "unknown",
  state: "unknown",
};

export function memberRow(
  n: number,
  overrides: Partial<GroupMemberRow> = {},
): GroupMemberRow {
  return {
    item: { resultVersion: RESULT, itemNo: n },
    displayName: { state: "present", value: `file-${n}.mp4` },
    displayPath: { state: "present", value: `C:\\Videos\\file-${n}.mp4` },
    logicalSize: { state: "present", value: exact("1048576") },
    allocatedSize: { state: "present", value: exact("1052672") },
    groups: [MOVIES],
    undecidableReasonKeys: [],
    ...overrides,
  };
}

const total = (bytes: string) => ({
  sizes: [
    {
      size: exact(bytes),
      completeness: "complete" as const,
      measurementLimits: [],
    },
  ],
  completeness: "complete" as const,
});

export function totals(overrides: Partial<GroupTotals> = {}): GroupTotals {
  return {
    classificationRef: CLASSIFICATION,
    resultVersion: RESULT,
    groups: [
      { groupId: MOVIES, total: total("3145728") },
      { groupId: ARCHIVE, total: total("5242880") },
    ],
    union: total("7340032"),
    overlap: total("1048576"),
    overlapCount: 1,
    unclassified: total("2097152"),
    undecidable: total("0"),
    scope: total("9437184"),
    ratios: [
      {
        state: "value",
        basis: "logical",
        numerator: "7340032",
        denominator: "9437184",
      },
    ],
    sizeRule: { id: "size", revision: 1 },
    ...overrides,
  };
}

/** The standard answers for a classification that returns a summary at once. */
export function standardHandlers(extra: Record<string, Handler> = {}) {
  return {
    "group.rules.get": () => ruleset(),
    "group.classify": (body: { requestSeq: number }) => ({
      kind: "summary",
      requestSeq: body.requestSeq,
      summary: summary(),
    }),
    "group.summary": () => summary(),
    "group.members": (body: { start: number; count: number }) => ({
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
      start: body.start,
      total: 3,
      rows: Array.from(
        { length: Math.min(body.count, 3 - body.start) },
        (_, i) => memberRow(body.start + i + 1),
      ),
      sort: { id: "size", revision: 1 },
    }),
    "group.totals": () => totals(),
    "group.explain": () => {
      throw new AnalysisError("NOT_FOUND", "ref.unavailable");
    },
    "group.cancel": () => ({ outcome: "accepted" }),
    "group.rules.save": (body: Body) => ({
      outcome: "saved",
      ruleset: ruleset({
        revision: body.expectedRevision + 1,
        rulesetVersion: 4,
        fingerprint: "dd",
      }),
    }),
    "job.list": () => [],
    ...extra,
  } satisfies Record<string, Handler>;
}
