import { expect, test } from "vitest";
import {
  GROUP_VIEW_TRANSITIONS,
  ViewChannel,
  isRequeryReason,
  judgeResponse,
  nextGroupViewPhase,
} from "../src/state/groups-view-machine";
import type {
  GroupViewEvent,
  GroupViewPhase,
  ViewCondition,
} from "../src/state/groups-view-machine";

const transitions: [GroupViewPhase, GroupViewEvent, GroupViewPhase][] = [
  ["idle", "request", "requesting"],
  ["requesting", "summary", "showing"],
  ["requesting", "job", "working"],
  ["requesting", "not_applicable", "not_applicable"],
  ["requesting", "request_failed", "showing"],
  ["requesting", "stale", "requery_needed"],
  ["working", "job_summary", "showing"],
  ["working", "job_ended", "showing"],
  ["working", "stale", "requery_needed"],
  ["showing", "request", "requesting"],
  ["showing", "version_changed", "updatable"],
  ["showing", "stale", "requery_needed"],
  ["showing", "rules_changed", "requery_needed"],
  ["updatable", "request", "requesting"],
  ["updatable", "stale", "requery_needed"],
  ["updatable", "rules_changed", "requery_needed"],
  ["requery_needed", "request", "requesting"],
  ["not_applicable", "request", "requesting"],
];

test.each(transitions)("F36 FR2: %s + %s -> %s", (from, event, to) => {
  expect(nextGroupViewPhase(from, event)).toBe(to);
});

test("F36 FR2: the table holds exactly the listed transitions and ignores every other event", () => {
  const listed = Object.values(GROUP_VIEW_TRANSITIONS).reduce(
    (sum, row) => sum + Object.keys(row).length,
    0,
  );
  expect(listed).toBe(transitions.length);
  // A classification cannot be requested while one is running.
  expect(nextGroupViewPhase("requesting", "request")).toBeUndefined();
  expect(nextGroupViewPhase("working", "request")).toBeUndefined();
  // A new result version only offers an update from a shown result.
  expect(nextGroupViewPhase("idle", "version_changed")).toBeUndefined();
  expect(nextGroupViewPhase("working", "version_changed")).toBeUndefined();
  expect(
    nextGroupViewPhase("requery_needed", "version_changed"),
  ).toBeUndefined();
});

test("F36 FR2: only stale reasons ask for a new request", () => {
  for (const key of [
    "result.version_stale",
    "result.requery_required",
    "group.result_expired",
    "group.rules_modified",
  ])
    expect(isRequeryReason(key)).toBe(true);
  for (const key of ["db.busy", "group.unknown", undefined])
    expect(isRequeryReason(key)).toBe(false);
});

const current: ViewCondition = {
  resultVersion: "v1",
  scope: undefined,
  rulesetVersion: 3,
  rulesetFingerprint: "aa",
};

test("F36 FR2: a channel issues strictly increasing sequences", () => {
  const channel = new ViewChannel("view");
  expect(channel.last).toBe(0);
  expect([channel.next(), channel.next(), channel.next()]).toEqual([1, 2, 3]);
  expect(channel.last).toBe(3);
});

test("F36 FR2: a response is judged by sequence, then version, then rule set", () => {
  const channel = new ViewChannel("view");
  channel.next();
  channel.next();
  const ok = {
    requestSeq: 2,
    resultVersion: "v1",
    rulesetVersion: 3,
    rulesetFingerprint: "aa",
  };
  expect(judgeResponse(channel, current, ok)).toBe("accept");
  // An older sequence is dropped before anything else is looked at.
  expect(
    judgeResponse(channel, current, {
      ...ok,
      requestSeq: 1,
      resultVersion: "other",
    }),
  ).toBe("superseded");
  expect(judgeResponse(channel, current, { ...ok, requestSeq: 3 })).toBe(
    "superseded",
  );
  // The same sequence with a different result version is dropped.
  expect(judgeResponse(channel, current, { ...ok, resultVersion: "v2" })).toBe(
    "mismatch",
  );
  // Likewise for the rule set version and its fingerprint.
  expect(judgeResponse(channel, current, { ...ok, rulesetVersion: 4 })).toBe(
    "mismatch",
  );
  expect(
    judgeResponse(channel, current, { ...ok, rulesetFingerprint: "bb" }),
  ).toBe("mismatch");
});

test("F36 FR2: an unpinned target accepts the first version it is given", () => {
  const channel = new ViewChannel("view");
  channel.next();
  const open = { ...current, resultVersion: undefined };
  expect(
    judgeResponse(channel, open, { requestSeq: 1, resultVersion: "v9" }),
  ).toBe("accept");
});
