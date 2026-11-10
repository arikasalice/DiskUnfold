import { expect, test } from "vitest";
import { displaySize } from "../src/features/analysis/space-format";
import type { SizeValue } from "../src/ipc";

function exact(bytes: string): SizeValue {
  return { basis: "logical", evidence: "measured", state: "exact", bytes };
}

test("F03: unit selection uses the original count and preserves unknown and zero", () => {
  expect(displaySize(exact("999"), { system: "si", unit: "auto" })).toBe(
    "999 bytes",
  );
  expect(displaySize(exact("1000"), { system: "si", unit: "auto" })).toBe(
    "1.0 kB",
  );
  expect(displaySize(exact("1023"))).toBe("1,023 bytes");
  expect(displaySize(exact("1024"))).toBe("1.0 KiB");
  expect(displaySize(exact("18446744073709551615"))).toBe("16,384.0 PiB");
  expect(displaySize(exact("0"))).toBe("0 bytes");
  expect(
    displaySize({ basis: "allocated", evidence: "unknown", state: "unknown" }),
  ).toBe("Unavailable");
});

test("F03: a displayed lower bound stays at or below the raw byte count", () => {
  const exactSize = exact("1100");
  expect(displaySize(exactSize)).toBe("1.1 KiB");
  expect(
    displaySize({ ...exactSize, state: "lower_bound", bytes: "1100" }),
  ).toBe("At least 1.0 KiB");
});
