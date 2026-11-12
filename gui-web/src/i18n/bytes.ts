import { displayInteger } from "./regional-format";
import type { RegionalFormat } from "./regional-format";

/** Unit selection used only to format the retained byte values. */
export type SizeDisplay = {
  system: "iec" | "si";
  unit: number | "auto";
  regional?: RegionalFormat;
};

type UnitTable = { base: number; symbols: readonly string[] };

function byteQuantity(
  bytes: bigint,
  table: UnitTable,
  precision: number,
  unit: number | "auto",
  lowerBound: boolean,
) {
  const base = BigInt(table.base);
  let index = unit === "auto" ? 0 : unit;
  let divisor = base ** BigInt(index);
  if (unit === "auto") {
    while (index + 1 < table.symbols.length && bytes >= divisor * base) {
      divisor *= base;
      index += 1;
    }
  }
  const scale = 10n ** BigInt(precision);
  // Rounding a bound upward would overstate the known minimum.
  const rounding = lowerBound ? 0n : divisor / 2n;
  const scaled = (bytes * scale + rounding) / divisor;
  return {
    index,
    integer: scaled / scale,
    remainder: scaled % scale,
    unitId: table.symbols[index] ?? "B",
  };
}

export function formatRegionalCapacity(
  bytes: bigint,
  table: UnitTable,
  precision: number,
  unit: number | "auto",
  lowerBound: boolean,
  regional: RegionalFormat,
) {
  const { integer, remainder, unitId } = byteQuantity(
    bytes,
    table,
    precision,
    unit,
    lowerBound,
  );
  const decimal =
    remainder === 0n
      ? ""
      : `${regional.decimalSeparator}${remainder.toString().padStart(precision, "0")}`;
  const visualNumber = `${displayInteger(integer, regional)}${decimal}`;
  return {
    visual: `${visualNumber} ${unitId}`,
    number: `${integer}${decimal}`,
    visualNumber,
    unitId,
  };
}

/** Converts unsigned byte counts using exact arithmetic until display. */
export function formatByteUnit(
  bytes: bigint,
  table: UnitTable,
  precision: number,
  unit: number | "auto",
  lowerBound: boolean,
  regional?: RegionalFormat,
): string {
  if (regional) {
    const value = formatRegionalCapacity(
      bytes,
      table,
      precision,
      unit,
      lowerBound,
      regional,
    );
    return value.unitId === "B" ? `${value.visualNumber} bytes` : value.visual;
  }
  const { index, integer, remainder, unitId } = byteQuantity(
    bytes,
    table,
    precision,
    unit,
    lowerBound,
  );
  if (index === 0) return `${displayInteger(bytes)} bytes`;
  const whole = displayInteger(integer);
  const fraction = new Intl.NumberFormat(undefined, {
    minimumIntegerDigits: precision,
    useGrouping: false,
  }).format(remainder);
  const separator = new Intl.NumberFormat()
    .formatToParts(0.1)
    .find(({ type }) => type === "decimal")?.value;
  return `${whole}${separator ?? "."}${fraction} ${unitId}`;
}
