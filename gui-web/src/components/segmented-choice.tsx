import { Radio, RadioGroup } from "react-aria-components";
import "../theme/segmented-choice.css";

export type SegmentedOption = {
  value: string;
  label: string;
  /** A short mark beside the name, also part of the option's name for assistive technology. */
  note?: string | undefined;
};

/**
 * One line of mutually exclusive options that behaves as a radio group: the
 * arrow keys move and select, and one option is always chosen. When the line is
 * wider than its place it scrolls sideways instead of wrapping (F06 README 3).
 * A noted option stays selectable; its note says what is special about it.
 */
export function SegmentedChoice({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: readonly SegmentedOption[];
  onChange: (value: string) => void;
}) {
  return (
    <RadioGroup
      aria-label={label}
      value={value}
      onChange={onChange}
      orientation="horizontal"
      className="du-segmented"
    >
      {options.map((option) => (
        <Radio
          key={option.value}
          value={option.value}
          className="du-segmented__option"
          {...(option.note
            ? { "aria-label": `${option.label}, ${option.note}` }
            : {})}
        >
          <span>{option.label}</span>
          {option.note && (
            <span className="du-segmented__note" aria-hidden>
              {option.note}
            </span>
          )}
        </Radio>
      ))}
    </RadioGroup>
  );
}
