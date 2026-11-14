import type { Ref } from "react";
import { Action, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { SampleSession } from "../../ipc";
import type { SampleCommand, SampleProblem } from "../../state/sample-session";
import {
  SampleProblemNotice,
  SampleProgress,
  SampleUnavailable,
} from "./SampleStates";

export type SampleEntryProps = {
  /** The held session while the sample result screen is not shown. */
  session: SampleSession | undefined;
  pending: SampleCommand | undefined;
  problem: SampleProblem | undefined;
  /** Example items prepared so far, when the host reports it. */
  preparedItems?: number | undefined;
  /** Receives the entry button, so the focus can return to it after the sample ends. */
  entryRef: Ref<HTMLButtonElement>;
  onStart: () => void;
  /** Prepares the sample again; it asks for no confirmation. */
  onRetry: () => void;
  /** Releases the session that is being prepared. */
  onRelease: () => void;
  onDismissProblem: () => void;
};

/** What the entry shows below the button: preparing, resetting or unavailable. */
function entryState(
  session: SampleSession | undefined,
  pending: SampleCommand | undefined,
) {
  if (pending === "open") return "preparing";
  if (pending === "reset") return "resetting";
  if (session?.state === "preparing") return "preparing";
  if (session?.state === "resetting") return "resetting";
  if (session?.state === "unavailable") return "unavailable";
  return undefined;
}

/**
 * Offers the sample experience on the analysis target screen and shows what
 * happens after the button is pressed, in the same place: progress with a
 * cancel button while the sample is prepared, a notice when it is unavailable,
 * and a failed request. Real analysis stays usable throughout.
 */
export function SampleEntry(props: SampleEntryProps) {
  const { session, pending, problem } = props;
  const text = useLocaleText();
  const state = entryState(session, pending);
  return (
    <div className="du-sample-entry">
      <div className="du-sample-entry__row">
        <Action
          ref={props.entryRef}
          icon="sample"
          isDisabled={pending !== undefined || session !== undefined}
          onPress={props.onStart}
        >
          {text("ui.sample.entry", "Try sample data first")}
        </Action>
        <Text kind="muted">
          {text(
            "ui.sample.entry_hint",
            "You can look around with example data first. Your disk is not analyzed.",
          )}
        </Text>
      </div>
      {(state === "preparing" || state === "resetting") && (
        <SampleProgress
          resetting={state === "resetting"}
          preparedItems={props.preparedItems}
          cancelDisabled={pending !== undefined || session === undefined}
          onCancel={props.onRelease}
        />
      )}
      {state === "unavailable" && (
        <SampleUnavailable
          reason={session?.unavailableReason}
          isDisabled={pending !== undefined}
          onRetry={props.onRetry}
        />
      )}
      <SampleProblemNotice
        problem={problem}
        onDismiss={props.onDismissProblem}
      />
    </div>
  );
}
