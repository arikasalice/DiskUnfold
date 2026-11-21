import { useId } from "react";
import { Radio } from "react-aria-components";
import { MiddleEllipsisText } from "./text-fit";
import { StatusBadge } from "./presentation";
import { Icon } from "./presentation";
import type { IconName } from "./presentation";
import "../theme/line-choice.css";

/**
 * A choice that stands on one line: marker, icon, the label cut in the middle, and
 * the status. The accessible name is the full label and the description stays
 * available to assistive technology, so what the line shortens is not lost (F02 #350).
 */
export function LineChoice({
  value,
  label,
  description,
  status,
  icon,
}: {
  value: string;
  label: string;
  description: string;
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
      className="du-choice du-line-choice"
    >
      <span className="du-choice__indicator" aria-hidden />
      <Icon name={icon} />
      <span className="du-line-choice__label du-text--muted">
        <MiddleEllipsisText text={label} cell />
      </span>
      <span className="du-line-choice__description" id={descriptionId}>
        {description}
      </span>
      <span className="du-choice__status" id={statusId}>
        <StatusBadge tone={status.tone}>{status.text}</StatusBadge>
      </span>
    </Radio>
  );
}
