/** Unit selection used only to format the retained byte values. */
export type SizeDisplay = { system: "iec" | "si"; unit: number | "auto" };

type UnitTable = { base: number; symbols: readonly string[] };

/** Converts unsigned byte counts using exact arithmetic until display. */
export function formatByteUnit(
  bytes: bigint,
  table: UnitTable,
  precision: number,
  unit: number | "auto",
  lowerBound: boolean,
): string {
  const base = BigInt(table.base);
  let index = unit === "auto" ? 0 : unit;
  let divisor = base ** BigInt(index);
  if (unit === "auto") {
    while (index + 1 < table.symbols.length && bytes >= divisor * base) {
      divisor *= base;
      index += 1;
    }
  }
  if (index === 0) return `${bytes.toLocaleString()} bytes`;
  const scale = 10n ** BigInt(precision);
  // Rounding a bound upward would overstate the known minimum.
  const rounding = lowerBound ? 0n : divisor / 2n;
  const scaled = (bytes * scale + rounding) / divisor;
  const whole = (scaled / scale).toLocaleString();
  const fraction = new Intl.NumberFormat(undefined, {
    minimumIntegerDigits: precision,
    useGrouping: false,
  }).format(scaled % scale);
  const separator = new Intl.NumberFormat()
    .formatToParts(0.1)
    .find(({ type }) => type === "decimal")?.value;
  return `${whole}${separator ?? "."}${fraction} ${table.symbols[index]}`;
}
