import type { Ref } from "react";
import { Action } from "../../components";
import { useLocaleText } from "../../i18n/presentation";

/** The secondary action of the browse screen that opens the virtual-group view. */
export function GroupsEntryAction({
  visible,
  trigger,
  onOpen,
}: {
  visible: boolean;
  trigger: Ref<HTMLButtonElement>;
  onOpen: () => void;
}) {
  const text = useLocaleText();
  if (!visible) return null;
  return (
    <Action ref={trigger} onPress={onOpen}>
      {text("ui.groups.open", "View as virtual groups")}
    </Action>
  );
}
