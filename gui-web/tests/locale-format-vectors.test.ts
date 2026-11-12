import { expect, test } from "vitest";
import vectors from "../../user-env/i18n/format-vectors.json";
import { formatRegionalCapacity } from "../src/i18n/bytes";
import {
  displayDecimal,
  displayInteger,
  displayTimestamp,
} from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";
import { SIZE_UNIT_RULES } from "../src/ipc";

test.each(vectors.cases.filter(({ raw }) => raw.state !== "unknown"))(
  "matches the shared Rust display and speech vector $id",
  ({ table, profile, raw, expected, localOffsetSeconds }) => {
    const symbols = table === "ko" ? vectors.tables.ko : vectors.tables.en;
    const dateFormat =
      profile.dateFormatId === "iso8601" ? "iso8601" : "lang_default";
    const format: RegionalFormat = {
      ...symbols,
      datePattern: symbols.datePatterns[dateFormat],
      dateFormat,
      groupNumbers: profile.numberFormatId !== "no_grouping",
      timeZonePolicy: profile.timeZonePolicy === "local" ? "local" : "utc",
      localTimeZoneStatus:
        localOffsetSeconds === null ? "unavailable" : "offset_only",
    };
    const spoken = { ...format, groupNumbers: false };
    if (raw.kind === "capacity" && typeof raw.value === "string") {
      const units =
        profile.sizeUnitSystem === "iec"
          ? SIZE_UNIT_RULES.iec
          : SIZE_UNIT_RULES.si;
      const selected = units.symbols.findIndex(
        (symbol) => symbol === profile.sizeUnit,
      );
      const result = formatRegionalCapacity(
        BigInt(raw.value),
        units,
        SIZE_UNIT_RULES.decimalPlaces,
        selected < 0 ? "auto" : selected,
        raw.state === "lower_bound",
        format,
      );
      expect({ kind: "literal", text: result.visual }).toEqual(expected.visual);
      expect({
        kind: "quantity",
        number: result.number,
        unitId: result.unitId,
      }).toEqual(expected.screenReader);
      return;
    }
    if (raw.kind === "count" && typeof raw.value === "string") {
      const count = BigInt(raw.value);
      expect({ kind: "literal", text: displayInteger(count, format) }).toEqual(
        expected.visual,
      );
      expect({ kind: "plain", text: displayInteger(count, spoken) }).toEqual(
        expected.screenReader,
      );
      return;
    }
    if (raw.kind === "number" && typeof raw.value === "object") {
      const coefficient = BigInt(raw.value.coefficient);
      expect({
        kind: "literal",
        text: displayDecimal(coefficient, raw.value.scale, format),
      }).toEqual(expected.visual);
      expect({
        kind: "plain",
        text: displayDecimal(coefficient, raw.value.scale, spoken),
      }).toEqual(expected.screenReader);
      return;
    }
    if (raw.kind === "time" && typeof raw.value === "string") {
      const rendered = displayTimestamp(
        Date.parse(raw.value),
        format,
        localOffsetSeconds,
      );
      expect({ kind: "literal", text: rendered?.text }).toEqual(
        expected.visual,
      );
      expect({
        kind: "time",
        date: rendered?.date,
        offsetSeconds: rendered?.offsetSeconds,
      }).toEqual(expected.screenReader);
      expect(rendered?.timezoneUnavailable).toBe(
        expected.markers.some(
          (marker) => marker.key === "locale.timezone_unavailable",
        ),
      );
      return;
    }
    throw new Error(`Unsupported formatter fixture row: ${raw.kind}`);
  },
);
