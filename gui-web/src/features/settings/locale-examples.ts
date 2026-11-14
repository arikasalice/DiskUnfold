import { displayDecimal, displayTimestamp } from "../../i18n/regional-format";
import type { RegionalFormat } from "../../i18n/regional-format";

/**
 * Sample lines for the format rows of the appearance and language settings
 * (F13 FR4.HD1.gui-web.DD1, FR6.HD1.gui-web.DD1).
 *
 * Every example is produced by the same formatters as the rest of the screens,
 * from the profile's format, so the sample shows exactly what the user gets.
 */

/** The number shown under the number format row. */
const SAMPLE_NUMBER = { coefficient: 12345n, scale: 1 } as const;

/** Formats 1234.5 with the separators and grouping of the current profile. */
export function numberExample(format: RegionalFormat): string {
  return displayDecimal(SAMPLE_NUMBER.coefficient, SAMPLE_NUMBER.scale, format);
}

/** Formats `now` with the current date pattern and time zone policy, or `undefined` when it cannot be shown. */
export function dateExample(
  now: number,
  format: RegionalFormat,
): string | undefined {
  return displayTimestamp(now, format)?.text;
}

/**
 * The UTC offset that the current time zone policy gives `now`, such as `UTC+09:00`.
 *
 * Returns `undefined` when the policy is local and the local time zone is not
 * known, because no offset is available to show.
 */
export function offsetExample(
  now: number,
  format: RegionalFormat,
): string | undefined {
  const shown = displayTimestamp(now, format);
  if (!shown || shown.timezoneUnavailable) return undefined;
  if (shown.offsetSeconds === 0) return "UTC";
  const minutes = Math.floor(Math.abs(shown.offsetSeconds) / 60);
  const pad = (value: number) => String(value).padStart(2, "0");
  const sign = shown.offsetSeconds < 0 ? "-" : "+";
  return `UTC${sign}${pad(Math.floor(minutes / 60))}:${pad(minutes % 60)}`;
}
