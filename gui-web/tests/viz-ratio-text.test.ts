import { expect, test } from "vitest";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { RegionalFormat } from "../src/i18n/regional-format";

const message =
  (language: "en" | "ko") =>
  (
    key: Parameters<typeof visualizationMessage>[1],
    values?: Record<string, string | bigint>,
  ) =>
    Object.entries(values ?? {}).reduce(
      (text, [name, value]) => text.replace(`{${name}}`, String(value)),
      visualizationMessage(language, key),
    );
const textFor = (language: "en" | "ko", regional?: RegionalFormat) =>
  createProductionVizText(message(language), {
    system: "iec",
    unit: "auto",
    ...(regional ? { regional } : {}),
  });
const comma = {
  decimalSeparator: ",",
  groupingSeparator: ".",
  groupingSize: 3,
  datePattern: "yyyy-MM-dd'T'HH:mm:ss",
  dateFormat: "iso8601",
  groupNumbers: false,
  timeZonePolicy: "utc",
  localTimeZoneStatus: "identified",
} satisfies RegionalFormat;

test.each([
  ["0", "0%", "0%"],
  ["0.00004", "Less than 0.1%", "0.1% 미만"],
  ["0.0005", "Less than 0.1%", "0.1% 미만"],
  ["0.0009999999999999999999", "Less than 0.1%", "0.1% 미만"],
  ["0.001", "0.1%", "0.1%"],
  ["0.5", "50.0%", "50.0%"],
  ["0.52022392576154932419", "52.0%", "52.0%"],
  ["0.0733810772644744805", "7.3%", "7.3%"],
  ["0.9994", "99.9%", "99.9%"],
  ["0.9995", "99.9%", "99.9%"],
  ["0.99999999999999999999", "99.9%", "99.9%"],
  ["1", "100%", "100%"],
  ["1.00000000000000000000", "100%", "100%"],
  ["1.5", "150.0%", "150.0%"],
  ["-0.25", "-25.0%", "-25.0%"],
])("the ratio %s reads %s in English and %s in Korean", (ratio, en, ko) => {
  expect(textFor("en").ratio!(ratio)).toBe(en);
  expect(textFor("ko").ratio!(ratio)).toBe(ko);
});

test("rounding uses the exact digits, never a floating point value", () => {
  // 0.12449999999999999999 is below the half, 0.1245 is exactly on it.
  expect(textFor("en").ratio!("0.12449999999999999999")).toBe("12.4%");
  expect(textFor("en").ratio!("0.1245")).toBe("12.5%");
  // A float would turn this into exactly 0.3 and round it up to 30.0%.
  expect(textFor("en").ratio!("0.29949999999999999999")).toBe("29.9%");
});

test("the decimal separator follows the regional format", () => {
  expect(textFor("ko", comma).ratio!("0.52022392576154932419")).toBe("52,0%");
  expect(textFor("ko", comma).ratio!("0.00004")).toBe("0,1% 미만");
  expect(textFor("en", comma).ratio!("0.00004")).toBe("Less than 0,1%");
});

test("a string that is not a canonical decimal gets the general phrase", () => {
  for (const value of ["abc", "1e-3", "0.", ".5", "1/3", ""])
    expect(textFor("en").ratio!(value)).toBe("Unknown");
});
