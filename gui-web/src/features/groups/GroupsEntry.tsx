import { useId } from "react";
import type { Ref } from "react";
import { Action, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";

/**
 * The secondary action of the browse screen that opens the virtual-group view.
 * For a sample result it is disabled but keeps focus (aria-disabled), and the
 * reason is always on screen beside it and is also its description.
 * TEMPORARY (F36 sample view, follow-up codex/f36-sample-rules): remove the
 * disabled state and its reason when that change ships.
 */
export function GroupsEntryAction({
  visible,
  sample,
  trigger,
  onOpen,
}: {
  visible: boolean;
  sample: boolean;
  trigger: Ref<HTMLButtonElement>;
  onOpen: () => void;
}) {
  const text = useLocaleText();
  const reasonId = useId();
  if (!visible) return null;
  if (sample)
    return (
      <div className="du-groups-entry" data-unavailable>
        <div id={reasonId} className="du-groups-entry__reason">
          <Text kind="muted">
            {text(
              "ui.groups.open_sample_reason",
              "Not available in the sample yet",
            )}
          </Text>
        </div>
        <Action
          ref={trigger}
          aria-disabled
          {...{ "aria-describedby": reasonId }}
          onPress={() => {}}
        >
          {text("ui.groups.open", "View as virtual groups")}
        </Action>
      </div>
    );
  return (
    <Action ref={trigger} onPress={onOpen}>
      {text("ui.groups.open", "View as virtual groups")}
    </Action>
  );
}
