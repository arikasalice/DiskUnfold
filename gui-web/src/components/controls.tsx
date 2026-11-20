import { useId, useRef, useState } from "react";
import type { ReactNode, Ref } from "react";
import { parseDate } from "@internationalized/date";
import {
  Button,
  Calendar,
  CalendarCell,
  CalendarGrid,
  CalendarGridBody,
  CalendarGridHeader,
  CalendarHeaderCell,
  CalendarHeading,
  DateInput,
  DatePicker as AriaDatePicker,
  DateSegment,
  Dialog,
  Group,
  Input,
  I18nProvider,
  Label,
  Popover,
  Radio,
  RadioGroup,
  Text,
  TextField,
  Tooltip,
  TooltipTrigger,
  VisuallyHidden,
} from "react-aria-components";
import { CalendarDays, ChevronLeft, ChevronRight } from "lucide-react";
import type { ButtonProps } from "react-aria-components";
import { Icon, StatusBadge } from "./presentation";
import type { IconName } from "./presentation";
import { isImeProcessingKey, useImeComposition } from "./ime-composition";
import { useLocaleText } from "../i18n/presentation";
import {
  dateInputLocale,
  dateInputSeparators,
  useRegionalFormat,
} from "../i18n/regional-format";

type ActionProps = Pick<
  ButtonProps,
  | "onPress"
  | "onFocus"
  | "onBlur"
  | "isDisabled"
  | "isPending"
  | "autoFocus"
  | "excludeFromTabOrder"
  | "aria-expanded"
  | "aria-controls"
  | "aria-disabled"
  | "aria-describedby"
> & {
  variant?: "primary" | "secondary" | "quiet";
  compact?: boolean;
  ref?: Ref<HTMLButtonElement>;
} & (
    | { children: ReactNode; icon?: IconName; "aria-label"?: string }
    | { children?: never; icon: IconName; "aria-label": string }
  );

/** A keyboard-accessible action with a consistent treatment for disabled state. */
export function Action({
  children,
  variant = "secondary",
  compact = false,
  icon,
  ...props
}: ActionProps) {
  return (
    <Button
      {...props}
      className={`du-action du-action--${variant}${children == null ? " du-action--icon" : ""}${compact ? " du-action--compact" : ""}`}
    >
      {icon && <Icon name={icon} />}
      {compact ? (
        <span className="du-action__caption">{children}</span>
      ) : (
        children
      )}
    </Button>
  );
}

/**
 * A localized date picker that accepts and returns `YYYY-MM-DD` or an empty
 * string. An optional error is shown and announced below the field.
 */
export function DateField({
  label,
  value,
  onChange,
  error,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  error?: string | undefined;
}) {
  const text = useLocaleText();
  const regional = useRegionalFormat();
  const labelId = useId();
  const errorId = useId();
  const calendarButtonLabelId = useId();
  const dateInput = useRef<HTMLDivElement>(null);
  const activeLanguage = regional?.language ?? document.documentElement.lang;
  const language = activeLanguage.toLowerCase().startsWith("ko") ? "ko" : "en";
  const locale = language === "ko" ? "ko-KR" : "en-US";
  const separators = dateInputSeparators(regional?.datePattern);
  const inputLocale = dateInputLocale(language, regional?.datePattern);
  const placeholders = {
    year: text("ui.date_picker.placeholder_year", "YYYY"),
    month: text("ui.date_picker.placeholder_month", "MM"),
    day: text("ui.date_picker.placeholder_day", "DD"),
  };
  const calendarLabel = text(
    "ui.date_picker.open_calendar",
    "Open calendar for {label}",
    { label },
  );
  const clearLabel = text("ui.date_picker.clear", "Clear {label}", { label });
  const previousMonth = text("ui.date_picker.previous_month", "Previous month");
  const nextMonth = text("ui.date_picker.next_month", "Next month");

  return (
    <I18nProvider locale={locale}>
      <div className="du-field du-date-picker">
        <Label id={labelId}>{label}</Label>
        <div
          className="du-date-picker__control"
          onKeyDownCapture={(event) => {
            if (event.key === "Enter" && isImeProcessingKey(event.nativeEvent))
              event.stopPropagation();
          }}
          data-clearable={value ? "" : undefined}
        >
          <AriaDatePicker
            className="du-date-picker__picker"
            aria-labelledby={labelId}
            {...(error !== undefined && { "aria-describedby": errorId })}
            value={value ? parseDate(value) : null}
            onChange={(date) => onChange(date?.toString() ?? "")}
            isInvalid={error !== undefined}
          >
            <Group className="du-date-picker__input">
              <I18nProvider locale={inputLocale}>
                <DateInput ref={dateInput} className="du-date-picker__segments">
                  {(segment) => {
                    const separator =
                      segment.type === "literal"
                        ? segment.text.trim() === "." &&
                          !segment.text.endsWith(" ")
                          ? ""
                          : (separators[0] ?? segment.text)
                        : undefined;
                    const placeholder =
                      segment.type === "year" ||
                      segment.type === "month" ||
                      segment.type === "day"
                        ? placeholders[segment.type]
                        : undefined;
                    return (
                      <DateSegment
                        segment={segment}
                        className="du-date-picker__segment"
                      >
                        {segment.type === "literal"
                          ? separator
                          : segment.isPlaceholder
                            ? placeholder
                            : undefined}
                      </DateSegment>
                    );
                  }}
                </DateInput>
              </I18nProvider>
              <TooltipTrigger delay={500}>
                <Button
                  slot="trigger"
                  className="du-date-picker__button"
                  aria-labelledby={calendarButtonLabelId}
                >
                  <VisuallyHidden id={calendarButtonLabelId}>
                    {calendarLabel}
                  </VisuallyHidden>
                  <CalendarDays aria-hidden focusable="false" size={18} />
                </Button>
                <Tooltip className="du-date-picker__tooltip">
                  {calendarLabel}
                </Tooltip>
              </TooltipTrigger>
            </Group>
            <Popover className="du-date-picker__popover">
              <Dialog className="du-date-picker__dialog">
                <Calendar className="du-date-picker__calendar">
                  <header className="du-date-picker__calendar-header">
                    <Button
                      slot="previous"
                      className="du-date-picker__button"
                      aria-label={previousMonth}
                    >
                      <ChevronLeft aria-hidden focusable="false" size={18} />
                    </Button>
                    <CalendarHeading className="du-date-picker__month" />
                    <Button
                      slot="next"
                      className="du-date-picker__button"
                      aria-label={nextMonth}
                    >
                      <ChevronRight aria-hidden focusable="false" size={18} />
                    </Button>
                  </header>
                  <CalendarGrid
                    className="du-date-picker__grid"
                    weekdayStyle="short"
                  >
                    <CalendarGridHeader>
                      {(day) => (
                        <CalendarHeaderCell className="du-date-picker__weekday">
                          {day}
                        </CalendarHeaderCell>
                      )}
                    </CalendarGridHeader>
                    <CalendarGridBody>
                      {(date) => (
                        <CalendarCell
                          date={date}
                          className="du-date-picker__day"
                        />
                      )}
                    </CalendarGridBody>
                  </CalendarGrid>
                </Calendar>
              </Dialog>
            </Popover>
          </AriaDatePicker>
          {value && (
            <Button
              className="du-date-picker__button du-date-picker__clear"
              aria-label={clearLabel}
              onPress={() => {
                onChange("");
                requestAnimationFrame(() =>
                  dateInput.current
                    ?.querySelector<HTMLElement>('[role="spinbutton"]')
                    ?.focus(),
                );
              }}
            >
              ×
            </Button>
          )}
        </div>
        {error !== undefined && (
          <Text id={errorId} role="alert" className="du-date-picker__error">
            {error}
          </Text>
        )}
      </div>
    </I18nProvider>
  );
}

/**
 * A labelled native-path input with optional focus on insertion.
 *
 * `error` is linked to the input as its error message and announced once when
 * it appears. While an error shows, the description stops being a live region,
 * so the same moment is not read twice. `onEnter` runs once for an Enter that
 * is not part of an IME composition.
 */
export function PathField({
  label,
  description,
  placeholder,
  value,
  onChange,
  isDisabled,
  autoFocus,
  onCompositionChange,
  onEnter,
  error,
  ref,
}: {
  label: string;
  description: string;
  placeholder?: string;
  value: string;
  onChange: (value: string) => void;
  isDisabled: boolean;
  autoFocus?: boolean;
  onCompositionChange?: (composing: boolean) => void;
  onEnter?: () => void;
  error?: string | undefined;
  ref?: Ref<HTMLInputElement>;
}) {
  const imeComposition = useImeComposition(onCompositionChange, {
    ...(onEnter && { onEnter }),
  });

  return (
    <TextField
      value={value}
      onChange={onChange}
      isDisabled={isDisabled}
      isInvalid={error !== undefined}
      className="du-field"
    >
      <Label>{label}</Label>
      <Input
        ref={ref}
        autoFocus={autoFocus}
        placeholder={placeholder ?? ""}
        spellCheck={false}
        autoComplete="off"
        {...imeComposition}
      />
      <Text
        slot="description"
        aria-live={error === undefined ? "polite" : "off"}
      >
        {description}
      </Text>
      {error !== undefined && (
        <Text slot="errorMessage" role="alert">
          {error}
        </Text>
      )}
    </TextField>
  );
}

/** Marks a native drop target and leaves its child input as the keyboard equivalent. */
export function FolderDropSurface({
  children,
  isDisabled,
}: {
  children: ReactNode;
  isDisabled: boolean;
}) {
  const [over, setOver] = useState(false);
  return (
    <div
      className="du-drop-surface"
      data-diskunfold-dropzone={isDisabled ? undefined : ""}
      data-drag-over={over && !isDisabled ? "" : undefined}
      onDragEnter={(event) => {
        if (!isDisabled && event.dataTransfer.types.includes("Files"))
          setOver(true);
      }}
      onDragLeave={(event) => {
        const target = event.relatedTarget;
        if (!(target instanceof Node) || !event.currentTarget.contains(target))
          setOver(false);
      }}
      onDrop={() => setOver(false)}
    >
      {children}
    </div>
  );
}

/** A labelled native selector for compact lists of predefined values. */
export function SelectField<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <label className="du-field">
      <span>{label}</span>
      <select
        value={value}
        onChange={(event) => {
          const option = options.find(
            ({ value }) => value === event.currentTarget.value,
          );
          if (option) onChange(option.value);
        }}
      >
        {options.map(({ value, label }) => (
          <option key={value} value={value}>
            {label}
          </option>
        ))}
      </select>
    </label>
  );
}

/** A labelled list of choices with arrow-key selection and shared focus styling. */
export function ChoiceGroup({
  id,
  label,
  value,
  onChange,
  isDisabled,
  children,
}: {
  id: string;
  label: string;
  value: string | null;
  onChange: (value: string) => void;
  isDisabled: boolean;
  children: ReactNode;
}) {
  return (
    <RadioGroup
      id={id}
      aria-label={label}
      value={value}
      onChange={onChange}
      isDisabled={isDisabled}
      className="du-choices"
    >
      {children}
    </RadioGroup>
  );
}

/** One choice row. The visual marker shares the radio's focus and selected state. */
export function Choice({
  value,
  label,
  description,
  summary,
  availablePercent,
  status,
  icon,
}: {
  value: string;
  label: string;
  description: string;
  summary?: string | undefined;
  availablePercent?: number | undefined;
  status: { text: string; tone: "neutral" | "good" };
  icon: IconName;
}) {
  const descriptionId = useId();
  const statusId = useId();
  return (
    <Radio
      value={value}
      aria-label={label}
      aria-describedby={`${descriptionId} ${statusId}`}
      className="du-choice"
      data-compact={summary !== undefined || undefined}
    >
      <span className="du-choice__indicator" aria-hidden />
      <Icon name={icon} />
      <span className="du-choice__label du-text--path">{label}</span>
      <span
        className="du-choice__description du-text--muted"
        id={descriptionId}
      >
        {description}
      </span>
      {summary !== undefined && (
        <span className="du-choice__summary" aria-hidden>
          {summary}
        </span>
      )}
      {availablePercent !== undefined && (
        <span className="du-choice__capacity" aria-hidden>
          <span style={{ inlineSize: `${availablePercent}%` }} />
        </span>
      )}
      <span className="du-choice__status" id={statusId}>
        <StatusBadge tone={status.tone}>{status.text}</StatusBadge>
      </span>
    </Radio>
  );
}
