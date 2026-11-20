import { useEffect, useRef } from "react";
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
 * wider than its place it scrolls sideways instead of wrapping, with a thin
 * scroll bar; a vertical wheel over it scrolls it sideways, and the chosen
 * option is always scrolled into view (F06 README 3, 41). A noted option stays
 * selectable; its note says what is special about it.
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
  const line = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = line.current;
    if (!element) return undefined;
    const onWheel = (event: WheelEvent) => {
      if (
        element.scrollWidth <= element.clientWidth ||
        Math.abs(event.deltaY) <= Math.abs(event.deltaX)
      )
        return;
      event.preventDefault();
      element.scrollLeft += event.deltaY;
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, []);
  useEffect(() => {
    line.current
      ?.querySelector("[data-selected]")
      ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [value]);
  return (
    <RadioGroup
      ref={line}
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
