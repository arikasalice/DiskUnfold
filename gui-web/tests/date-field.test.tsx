import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { DateField } from "../src/components";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import {
  dateInputLocale,
  dateInputSeparators,
  RegionalFormatContext,
} from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";

afterEach(() => {
  cleanup();
  document.documentElement.lang = "";
});

const regional: RegionalFormat = {
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default",
  groupNumbers: true,
  timeZonePolicy: "local",
  localTimeZoneStatus: "identified",
};

const koreanMessages: Record<string, string> = {
  "ui.date_picker.clear": "{label} 지우기",
  "ui.date_picker.next_month": "다음 달",
  "ui.date_picker.open_calendar": "{label} 달력 열기",
  "ui.date_picker.placeholder_day": "일",
  "ui.date_picker.placeholder_month": "월",
  "ui.date_picker.placeholder_year": "연도",
  "ui.date_picker.previous_month": "이전 달",
};

const koreanText: LocaleText = (key, fallback, values) =>
  (koreanMessages[key] ?? fallback).replace(
    /\{label\}/g,
    String(values?.label ?? ""),
  );

const englishText: LocaleText = (_key, fallback, values) =>
  fallback.replace(/\{label\}/g, String(values?.label ?? ""));

function renderDateField({
  language = "en",
  value = "",
  onChange = vi.fn(),
}: {
  language?: "en" | "ko";
  value?: string;
  onChange?: (value: string) => void;
} = {}) {
  document.documentElement.lang = language;
  const label = language === "ko" ? "시작 날짜" : "Start date";
  return {
    onChange,
    ...render(
      <LocaleTextContext.Provider
        value={language === "ko" ? koreanText : englishText}
      >
        <RegionalFormatContext.Provider value={regional}>
          <DateField label={label} value={value} onChange={onChange} />
        </RegionalFormatContext.Provider>
      </LocaleTextContext.Provider>,
    ),
  };
}

test("the calendar button name and tooltip use the app language", async () => {
  renderDateField();
  const button = screen.getByRole("button", {
    name: "Open calendar for Start date",
  });
  fireEvent.mouseEnter(button);
  expect((await screen.findByRole("tooltip")).textContent).toBe(
    "Open calendar for Start date",
  );
});

test("the Korean date field localizes its calendar button and empty segments", () => {
  renderDateField({ language: "ko" });
  expect(
    screen.getByRole("button", { name: "시작 날짜 달력 열기" }),
  ).toBeDefined();
  expect(screen.getByText("연도")).toBeDefined();
  expect(screen.getByText("월")).toBeDefined();
  expect(screen.getByText("일")).toBeDefined();
});

test("clearing a value reports an empty date and focuses the first segment", async () => {
  const onChange = vi.fn();
  renderDateField({ language: "ko", value: "2026-11-20", onChange });
  fireEvent.click(screen.getByRole("button", { name: "시작 날짜 지우기" }));
  expect(onChange).toHaveBeenCalledWith("");
  await waitFor(() =>
    expect(document.activeElement).toBe(screen.getAllByRole("spinbutton")[0]),
  );
});

test("selecting a calendar date round trips the ISO date string", async () => {
  const onChange = vi.fn();
  renderDateField({ value: "2026-11-19", onChange });
  fireEvent.click(
    screen.getByRole("button", { name: "Open calendar for Start date" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /November 20, 2026/ }),
  );
  expect(onChange).toHaveBeenCalledWith("2026-11-20");
});

test("the date format chooses matching segment order and separators", () => {
  expect(dateInputLocale("ko", "yyyy.MM.dd HH:mm:ss")).toBe("ko-KR");
  expect(dateInputLocale("en", "MM/dd/yyyy HH:mm:ss")).toBe("en-US");
  expect(dateInputLocale("en", "yyyy-MM-dd'T'HH:mm:ss")).toBe("en-CA");
  expect(dateInputSeparators("yyyy.MM.dd HH:mm:ss")).toEqual([".", "."]);
  expect(dateInputSeparators("yyyy-MM-dd'T'HH:mm:ss")).toEqual(["-", "-"]);
});
