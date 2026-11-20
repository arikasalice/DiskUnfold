import { DateField } from "../src/components";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";

const regional: RegionalFormat = {
  language: "ko",
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default",
  groupNumbers: true,
  timeZonePolicy: "local",
  localTimeZoneStatus: "identified",
};

const messages: Record<string, string> = {
  "ui.date_picker.clear": "{label} 지우기",
  "ui.date_picker.next_month": "다음 달",
  "ui.date_picker.open_calendar": "{label} 달력 열기",
  "ui.date_picker.placeholder_day": "일",
  "ui.date_picker.placeholder_month": "월",
  "ui.date_picker.placeholder_year": "연도",
  "ui.date_picker.previous_month": "이전 달",
  "ui.history.invalid_date_range": "시작 날짜가 끝 날짜보다 뒤입니다.",
};

const text: LocaleText = (key, fallback, values) =>
  (messages[key] ?? fallback).replace(
    /\{label\}/g,
    String(values?.label ?? ""),
  );

/** Shows the shared date field in empty, selected, open, and invalid states. */
export function DatePickerPreview() {
  return (
    <LocaleTextContext.Provider value={text}>
      <RegionalFormatContext.Provider value={regional}>
        <div className="du-date-picker-preview">
          <DateField label="날짜" value="" onChange={() => {}} />
          <DateField
            label="선택한 날짜"
            value="2026-11-20"
            onChange={() => {}}
          />
          <DateField
            label="달력 미리보기"
            value="2026-11-20"
            onChange={() => {}}
          />
          <DateField
            label="시작 날짜"
            value="2026-11-21"
            onChange={() => {}}
            error={text("ui.history.invalid_date_range", "")}
          />
        </div>
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
