import { useId, useState } from "react";
import type { ReactNode, Ref } from "react";
import {
  Button,
  Input,
  Label,
  Radio,
  RadioGroup,
  Text,
  TextField,
} from "react-aria-components";
import type { ButtonProps } from "react-aria-components";
import { Icon, StatusBadge } from "./presentation";
import type { IconName } from "./presentation";

type ActionProps = Pick<
  ButtonProps,
  "onPress" | "isDisabled" | "isPending" | "autoFocus"
> & {
  variant?: "primary" | "secondary" | "quiet";
  ref?: Ref<HTMLButtonElement>;
} & (
    | { children: ReactNode; icon?: IconName; "aria-label"?: string }
    | { children?: never; icon: IconName; "aria-label": string }
  );

/** A keyboard-accessible action with a consistent treatment for disabled state. */
export function Action({
  children,
  variant = "secondary",
  icon,
  ...props
}: ActionProps) {
  return (
    <Button
      {...props}
      className={`du-action du-action--${variant}${children == null ? " du-action--icon" : ""}`}
    >
      {icon && <Icon name={icon} />}
      {children}
    </Button>
  );
}

/** A labelled date input using the browser's local calendar. */
export function DateField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <TextField value={value} onChange={onChange} className="du-field">
      <Label>{label}</Label>
      <Input type="date" />
    </TextField>
  );
}

/** A labelled native-path input with optional focus on insertion. */
export function PathField({
  label,
  description,
  placeholder,
  value,
  onChange,
  isDisabled,
  autoFocus,
}: {
  label: string;
  description: string;
  placeholder?: string;
  value: string;
  onChange: (value: string) => void;
  isDisabled: boolean;
  autoFocus?: boolean;
}) {
  return (
    <TextField
      value={value}
      onChange={onChange}
      isDisabled={isDisabled}
      className="du-field"
    >
      <Label>{label}</Label>
      <Input
        autoFocus={autoFocus}
        placeholder={placeholder ?? ""}
        spellCheck={false}
        autoComplete="off"
      />
      <Text slot="description" aria-live="polite">
        {description}
      </Text>
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
  label,
  value,
  onChange,
  isDisabled,
  children,
}: {
  label: string;
  value: string | null;
  onChange: (value: string) => void;
  isDisabled: boolean;
  children: ReactNode;
}) {
  return (
    <RadioGroup
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
      className="du-choice"
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
      <span className="du-choice__status" id={statusId}>
        <StatusBadge tone={status.tone}>{status.text}</StatusBadge>
      </span>
    </Radio>
  );
}
