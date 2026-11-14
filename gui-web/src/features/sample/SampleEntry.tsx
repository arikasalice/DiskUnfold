import { Action, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";

/** Offers the sample experience from the analysis target screen. */
export function SampleEntry({
  isDisabled,
  onStart,
}: {
  isDisabled: boolean;
  onStart: () => void;
}) {
  const text = useLocaleText();
  return (
    <div className="du-sample-entry">
      <Text kind="muted">
        {text(
          "ui.sample.entry_hint",
          "Look around with example data first. Nothing on your disk is scanned.",
        )}
      </Text>
      <Action icon="sample" isDisabled={isDisabled} onPress={onStart}>
        {text("ui.sample.entry", "Try sample data first")}
      </Action>
    </div>
  );
}
