import { createContext, useContext } from "react";
import { useLocaleText } from "./presentation";

export type RegionalFormat = {
  decimalSeparator: string;
  groupingSeparator: string;
  groupingSize: number;
  datePattern: string;
  dateFormat: "lang_default" | "iso8601";
  groupNumbers: boolean;
  timeZonePolicy: "local" | "utc";
  localTimeZoneStatus: "identified" | "offset_only" | "unavailable";
  byteUnitText?: string;
};

export const RegionalFormatContext = createContext<RegionalFormat | undefined>(
  undefined,
);

export function useRegionalFormat() {
  return useContext(RegionalFormatContext);
}

export function displayInteger(value: bigint, format?: RegionalFormat): string {
  if (format === undefined) return value.toLocaleString();
  const raw = value.toString();
  if (!format.groupNumbers || format.groupingSize < 1) return raw;
  const negative = raw.startsWith("-");
  const digits = negative ? raw.slice(1) : raw;
  const chunks: string[] = [];
  for (let end = digits.length; end > 0; end -= format.groupingSize) {
    chunks.push(digits.slice(Math.max(0, end - format.groupingSize), end));
  }
  return `${negative ? "-" : ""}${chunks.reverse().join(format.groupingSeparator)}`;
}

export function displayDecimal(
  coefficient: bigint,
  scale: number,
  format: RegionalFormat,
): string {
  const negative = coefficient < 0n;
  const magnitude = negative ? -coefficient : coefficient;
  const divisor = 10n ** BigInt(Math.max(0, scale - 1));
  const tenths =
    scale === 0 ? magnitude * 10n : (magnitude + divisor / 2n) / divisor;
  const sign = negative && tenths !== 0n ? "-" : "";
  const fraction = tenths % 10n;
  return `${sign}${displayInteger(tenths / 10n, format)}${fraction === 0n ? "" : `${format.decimalSeparator}${fraction}`}`;
}

function pad(value: number, length = 2): string {
  return String(value).padStart(length, "0");
}

/** Formats the same timestamp without replacing the value used for sorting. */
export function displayTimestamp(
  value: number,
  format: RegionalFormat,
  observedOffsetSeconds?: number | null,
) {
  const source = new Date(value);
  if (!Number.isFinite(source.getTime())) return undefined;
  const timezoneUnavailable =
    format.timeZonePolicy === "local" &&
    (observedOffsetSeconds === null ||
      format.localTimeZoneStatus === "unavailable");
  const offset =
    format.timeZonePolicy === "local" && !timezoneUnavailable
      ? (observedOffsetSeconds ?? -source.getTimezoneOffset() * 60)
      : 0;
  const date = new Date(value + offset * 1000);
  if (!Number.isFinite(date.getTime())) return undefined;
  const fields: Record<string, string> = {
    yyyy: pad(date.getUTCFullYear(), 4),
    MM: pad(date.getUTCMonth() + 1),
    dd: pad(date.getUTCDate()),
    HH: pad(date.getUTCHours()),
    mm: pad(date.getUTCMinutes()),
    ss: pad(date.getUTCSeconds()),
    "'T'": "T",
  };
  const time = format.datePattern.replace(
    /yyyy|MM|dd|HH|mm|ss|'T'/g,
    (field) => fields[field] ?? field,
  );
  const minutes = Math.floor(Math.abs(offset) / 60);
  const zone = `${offset < 0 ? "-" : "+"}${pad(Math.floor(minutes / 60))}:${pad(minutes % 60)}`;
  const suffix =
    format.dateFormat === "iso8601"
      ? offset === 0
        ? "Z"
        : zone
      : offset === 0
        ? " UTC"
        : ` UTC${zone}`;
  return {
    text: time + suffix,
    date: time,
    offsetSeconds: offset,
    timezoneUnavailable,
  };
}

export function useTimestampText() {
  const regional = useRegionalFormat();
  const text = useLocaleText();
  return (value: number, fallback: () => string) => {
    const rendered = regional && displayTimestamp(value, regional);
    if (!rendered) return fallback();
    return rendered.timezoneUnavailable
      ? `${rendered.text} · ${text("ui.locale.time_zone_unavailable", "Local time zone unavailable")}`
      : rendered.text;
  };
}
