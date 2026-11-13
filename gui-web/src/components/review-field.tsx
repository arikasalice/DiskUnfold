import { Checkbox, Input, Label, Text, TextField } from "react-aria-components";
import { useImeComposition } from "./ime-composition";

export function ReviewField({
  label,
  value,
  onChange,
  description,
  isDisabled = false,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  description: string;
  isDisabled?: boolean;
}) {
  const imeComposition = useImeComposition();

  return (
    <TextField
      className="du-field"
      value={value}
      onChange={onChange}
      isDisabled={isDisabled}
    >
      <Label>{label}</Label>
      <Input {...imeComposition} />
      <Text slot="description">{description}</Text>
    </TextField>
  );
}

export function ReviewCheck({
  label,
  checked,
  onChange,
  isDisabled = false,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  isDisabled?: boolean;
}) {
  return (
    <Checkbox
      className="du-review-check"
      isSelected={checked}
      onChange={onChange}
      isDisabled={isDisabled}
    >
      <span className="du-review-check__mark" aria-hidden="true">
        {checked ? "✓" : ""}
      </span>
      {label}
    </Checkbox>
  );
}
