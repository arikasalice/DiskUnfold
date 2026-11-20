import { expect, test } from "vitest";
import { unixPath } from "../src/ipc";
import type { AnalysisTarget, TargetProbeEvent } from "../src/ipc";
import {
  ACCESS_NOT_CHECKED,
  defaultListing,
  shownAccess,
} from "../src/features/analysis/target-access";

const target = (
  raw: string,
  extra: Partial<AnalysisTarget> = {},
): AnalysisTarget => ({
  kind: "internal",
  path: unixPath(raw),
  connected: true,
  volumeId: null,
  identitySource: "unavailable",
  access: "checking",
  reasonKey: null,
  ...extra,
});
const root = target("/");
const efi = target("/boot/efi", { defaultVisible: false });
const snap = target("/snap/core/1", { defaultVisible: false });
const legacy = target("/mnt/data");
const network = target("/Volumes/share", { kind: "network_mount" });
const observedAt = "2026-11-20T00:00:00.000Z";
const unchecked: TargetProbeEvent = {
  target: network.path,
  sequence: 1,
  access: "unknown",
  reasonKey: ACCESS_NOT_CHECKED,
  totalBytes: "100",
  availableBytes: "40",
  capacityReason: null,
  aggregationUncertain: true,
  observedAt,
};
const idle = { selected: false, reviewing: false, reviewed: undefined };

test("F01 #263: hidden volumes leave the default list until all are shown", () => {
  const entries = [root, efi, snap, legacy];
  const folded = defaultListing(entries, undefined, false);
  expect(folded.listed).toEqual([root, legacy]);
  expect(folded.hidden).toBe(2);
  expect(defaultListing(entries, undefined, true).listed).toEqual(entries);
  expect(defaultListing(entries, efi.path, false)).toEqual({
    listed: [root, efi, legacy],
    hidden: 1,
  });
});

test("F01 #266: a network volume waits for selection, then shows its review", () => {
  expect(shownAccess(network, unchecked, true, idle)).toEqual({
    access: "not_checked",
    deferred: true,
  });
  const selected = { ...idle, selected: true };
  expect(
    shownAccess(network, unchecked, true, { ...selected, reviewing: true }),
  ).toEqual({ access: "checking", deferred: true });
  const reviewed = [
    { path: network.path, access: "denied" as const, reasonKey: null },
  ];
  expect(
    shownAccess(network, unchecked, true, { ...selected, reviewed }),
  ).toEqual({ access: "denied", deferred: true });
  expect(shownAccess(network, unchecked, true, selected).access).toBe(
    "not_checked",
  );
});

test("F01 #266: a probed volume keeps its own observation", () => {
  const denied = { ...unchecked, access: "denied" as const, reasonKey: null };
  expect(shownAccess(network, denied, true, idle)).toEqual({
    access: "denied",
    deferred: false,
  });
  expect(shownAccess(root, undefined, true, idle).access).toBe("unknown");
  expect(shownAccess(root, undefined, false, idle).access).toBe("checking");
});
