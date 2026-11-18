import type { VizText } from "./model-text";

/** The engine's canonical signed decimal: digits with an optional fraction. */
const CANONICAL = /^(-?)(\d+)(?:\.(\d+))?$/;
/** One tenth of a percent is 1 in 1000. */
const TENTHS_PER_UNIT = 1000n;

/**
 * Words a canonical ratio string as a percentage with one decimal, for example
 * "0.52022392576154932419" as "52.0%". The string is read as an integer and a
 * scale, never as a floating point number, so a ratio with twenty digits rounds
 * on its exact value: half away from zero at the tenth of a percent.
 *
 * An exact 0 reads "0%" and an exact 1 reads "100%". A positive value below
 * 0.1% reads "Less than 0.1%" instead of a misleading "0.0%"; a small negative
 * value reads as strictly between -0.1% and 0%. A value below 1 never rounds
 * up to "100%": it stops at "99.9%". Returns `null` when the string is not a
 * canonical decimal.
 */
export function formatRatio(
  value: string,
  message: VizText["message"],
  decimalSeparator = ".",
): string | null {
  const match = CANONICAL.exec(value);
  if (!match) return null;
  const [, sign, whole, fraction = ""] = match;
  const numerator = BigInt(whole! + fraction);
  if (sign && numerator === 0n) return null;
  const scale = 10n ** BigInt(fraction.length);
  const percent = (shown: string) => message("ratioPercent", { value: shown });
  if (numerator === 0n) return percent("0");
  if (numerator === scale) return percent(`${sign}100`);
  if (numerator * TENTHS_PER_UNIT < scale)
    return sign
      ? message("ratioNegativeBelowMinimum", {
          lower: percent(`-0${decimalSeparator}1`),
          upper: percent("0"),
        })
      : message("ratioBelowMinimum", {
          value: percent(`0${decimalSeparator}1`),
        });
  let tenths = (numerator * 2n * TENTHS_PER_UNIT + scale) / (2n * scale); // half up
  if (numerator < scale && tenths >= TENTHS_PER_UNIT)
    tenths = TENTHS_PER_UNIT - 1n;
  return percent(`${sign}${tenths / 10n}${decimalSeparator}${tenths % 10n}`);
}
