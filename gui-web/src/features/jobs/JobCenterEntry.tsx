import type { RefObject } from "react";
import { Action, CountPill } from "../../components";
import { useLocaleText } from "../../i18n/presentation";

/** The app-header action that opens or closes the Job Center. */
export function JobCenterEntry({
  clientAvailable,
  historyOpen,
  count,
  expanded,
  trigger,
  onPress,
}: {
  clientAvailable: boolean;
  historyOpen: boolean;
  count: number;
  expanded: boolean;
  trigger: RefObject<HTMLButtonElement | null>;
  onPress: () => void;
}) {
  const text = useLocaleText();
  if (!clientAvailable || historyOpen) return null;

  return (
    <Action
      variant="quiet"
      icon="activity"
      ref={trigger}
      aria-expanded={expanded}
      aria-controls="du-job-center-panel"
      {...(count > 0
        ? {
            "aria-label": text(
              "ui.job_center.header_name",
              "Tasks, {count} in progress",
              { count: BigInt(count) },
            ),
          }
        : {})}
      onPress={onPress}
    >
      {text("ui.job_center.tasks", "Tasks")}
      {count > 0 && <CountPill count={count} />}
    </Action>
  );
}
