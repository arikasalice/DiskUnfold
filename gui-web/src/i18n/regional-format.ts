import { createContext, useContext } from "react";
import { useLocaleText } from "./presentation";

export type RegionalFormat = {
  language?: string;
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

/** Returns a React Aria locale whose date segment order matches the setting. */
export function dateInputLocale(language: string, pattern?: string): string {
  const fields = [...(pattern?.matchAll(/yyyy|MM|dd/g) ?? [])]
    .slice(0, 3)
    .map((field) => field[0])
    .join("-");
  if (fields === "yyyy-MM-dd")
    return language.startsWith("ko") ? "ko-KR" : "en-CA";
  if (fields === "MM-dd-yyyy") return "en-US";
  if (fields === "dd-MM-yyyy") return "en-GB";
  return language.startsWith("ko") ? "ko-KR" : "en-US";
}

/** Returns the two literal separators in the date part of a regional pattern. */
export function dateInputSeparators(pattern?: string): string[] {
  const source = pattern ?? "";
  const fields = [...source.matchAll(/yyyy|MM|dd/g)].slice(0, 3);
  if (fields.length !== 3) return [];
  return [0, 1].map((index) => {
    const current = fields[index];
    const next = fields[index + 1];
    if (current?.index === undefined || next?.index === undefined) return "";
    return source.slice(current.index + current[0].length, next.index);
  });
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

/**
 * A displayed timestamp in the pieces a line may break between: the date
 * with its time, the time zone when it stands apart (" UTC+09:00") and a
 * note. An ISO 8601 zone joins the time, so it has no separate `zone`.
 */
export type TimestampParts = { moment: string; zone?: string; note?: string };

export function useTimestampParts() {
  const regional = useRegionalFormat();
  const text = useLocaleText();
  return (value: number, fallback: () => string): TimestampParts => {
    const rendered = regional && displayTimestamp(value, regional);
    if (!rendered) return { moment: fallback() };
    const suffix = rendered.text.slice(rendered.date.length);
    const parts: TimestampParts = suffix.startsWith(" ")
      ? { moment: rendered.date, zone: suffix.slice(1) }
      : { moment: rendered.text };
    return rendered.timezoneUnavailable
      ? {
          ...parts,
          note: text(
            "ui.locale.time_zone_unavailable",
            "Local time zone unavailable",
          ),
        }
      : parts;
  };
}

/** The same timestamp as one string, for text that is not drawn in pieces. */
export function timestampString({ moment, zone, note }: TimestampParts) {
  return `${moment}${zone === undefined ? "" : ` ${zone}`}${note === undefined ? "" : ` · ${note}`}`;
}

export function useTimestampText() {
  const parts = useTimestampParts();
  return (value: number, fallback: () => string) =>
    timestampString(parts(value, fallback));
}

/** The day of a timestamp in the app's pattern, without the time of day. */
export function displayDay(value: number, format: RegionalFormat) {
  const clock = format.datePattern.search(/'T'|HH/);
  const datePattern =
    clock > 0 ? format.datePattern.slice(0, clock).trim() : format.datePattern;
  return displayTimestamp(value, { ...format, datePattern })?.date;
}
