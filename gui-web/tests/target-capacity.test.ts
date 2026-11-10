import { expect, test } from "vitest";
import { describeTargetCapacity } from "../src/features/analysis/target-capacity";
import { windowsPath } from "../src/ipc";

test.each([
  [null, "10", undefined],
  ["10", null, undefined],
  ["0", "0", undefined],
  ["10", "11", undefined],
  ["10", "0", 0],
  ["10", "10", 100],
  ["100", "25", 25],
  ["18446744073709551615", "9223372036854775808", 50],
])(
  "shows a capacity mark for valid observations (%s total, %s available)",
  (totalBytes, availableBytes, expected) => {
    const observedAt = "2026-11-11T00:00:00Z";
    const capacity = describeTargetCapacity(
      {
        sequence: 1,
        target: windowsPath("C:\\"),
        access: "accessible",
        reasonKey: null,
        totalBytes,
        availableBytes,
        capacityReason: null,
        aggregationUncertain: false,
        observedAt,
      },
      false,
    );

    expect(capacity.availablePercent).toBe(expected);
    expect(capacity.summary).toMatch(/^Available /);
    expect(capacity.evidence).toContain("Checked: " + observedAt);
  },
);
