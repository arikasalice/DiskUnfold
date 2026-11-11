import { expect, test } from "vitest";
import {
  displaySize,
  displaySizeParts,
  displaySizeWithBytes,
} from "../src/features/analysis/space-format";
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

test("F03 FR12: inspection retains original byte counts and value states", () => {
  const display = { system: "iec", unit: "auto" } as const;
  expect(displaySizeWithBytes(exact("1100"), display)).toBe(
    "1.1 KiB · 1,100 bytes",
  );
  expect(displaySizeWithBytes(exact("0"), display)).toBe("0 bytes");
  expect(
    displaySizeWithBytes(
      { ...exact("1100"), state: "lower_bound", bytes: "1100" },
      display,
    ),
  ).toBe("At least 1.0 KiB · At least 1,100 bytes");
  expect(
    displaySizeWithBytes(
      { basis: "allocated", evidence: "unknown", state: "unknown" },
      display,
    ),
  ).toBe("Unavailable");
  expect(
    displaySizeWithBytes(exact("18446744073709551615"), display),
  ).toContain("18,446,744,073,709,551,615 bytes");
});

test("inspection separates raw bytes only when the displayed value differs", () => {
  const display = { system: "iec", unit: "auto" } as const;
  expect(displaySizeParts(exact("0"), display)).toEqual({
    formatted: "0 bytes",
    raw: undefined,
  });
  expect(displaySizeParts(exact("999"), display)).toEqual({
    formatted: "999 bytes",
    raw: undefined,
  });
  expect(displaySizeParts(exact("1100"), display)).toEqual({
    formatted: "1.1 KiB",
    raw: "1,100 bytes",
  });
  expect(
    displaySizeParts(
      { ...exact("1100"), state: "lower_bound", bytes: "1100" },
      display,
    ),
  ).toEqual({ formatted: "At least 1.0 KiB", raw: "At least 1,100 bytes" });
  expect(
    displaySizeParts(
      { ...exact("999"), state: "lower_bound", bytes: "999" },
      display,
    ),
  ).toEqual({ formatted: "At least 999 bytes", raw: undefined });
});
