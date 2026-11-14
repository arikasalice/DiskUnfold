import { Action, Heading, Notice, Progress, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { SampleProblem } from "../../state/sample-session";
import { problemText, unavailableText } from "./sample-text";

/**
 * Progress while the sample is being prepared or reset: a title, the number
 * of example items prepared when it is known, and an indeterminate bar (the
 * total is not known). The words are not a live region: the sample announcer
 * reports each state change once.
 */
export function SampleProgress({
  resetting,
  preparedItems,
  onCancel,
  cancelDisabled = false,
}: {
  resetting: boolean;
  /** Example items prepared so far; omitted while it is not known. */
  preparedItems?: number | undefined;
  /** Offers "Cancel experience" when the preparation can be cancelled. */
  onCancel?: () => void;
  cancelDisabled?: boolean;
}) {
  const text = useLocaleText();
  const label = resetting
    ? text("ui.sample.resetting", "Resetting the sample experience")
    : text("ui.sample.preparing", "Preparing the sample data");
  return (
    <div className="du-sample__progress">
      <div className="du-sample__progress-body">
        <Heading level={3}>{label}</Heading>
        {preparedItems !== undefined && (
          <Text kind="muted">
            {text(
              "ui.sample.prepared_items",
              preparedItems === 1
                ? "{count} example item prepared"
                : "{count} example items prepared",
              { count: BigInt(preparedItems) },
            )}
          </Text>
        )}
        <Progress label={label} />
      </div>
      {onCancel && (
        <Action isDisabled={cancelDisabled} onPress={onCancel}>
          {text("ui.sample.cancel_preparing", "Cancel experience")}
        </Action>
      )}
    </div>
  );
}

/**
 * Tells, at the entry button, that the sample cannot be used. A sample that
 * this installation does not carry is an information notice with no action.
 * Any other cause is an error notice that says real analysis still works and
 * offers retry, which asks for no confirmation.
 */
export function SampleUnavailable({
  reason,
  isDisabled,
  onRetry,
}: {
  reason: string | undefined;
  isDisabled: boolean;
  onRetry: () => void;
}) {
  const text = useLocaleText();
  const missing = reason === "sample.missing";
  return (
    <section
      className="du-sample__unavailable"
      aria-label={text(
        "ui.sample.unavailable_title",
        "The sample experience is unavailable",
      )}
    >
      {/* The sample announcer reports the state, so the notice stays silent. */}
      <Notice tone={missing ? "info" : "error"} announce={false}>
        <div className="du-sample__notice-row">
          <Text>
            {missing
              ? unavailableText(reason, text)
              : `${text("ui.sample.open_failed", "The sample cannot be opened")} · ${unavailableText(reason, text)} ${text("ui.sample.real_still_available", "You can still run a real analysis.")}`}
          </Text>
          {!missing && (
            <Action isDisabled={isDisabled} onPress={onRetry}>
              {text("ui.sample.retry", "Try again")}
            </Action>
          )}
        </div>
      </Notice>
    </section>
  );
}

/** Reports a failed sample command and lets the user dismiss it. */
export function SampleProblemNotice({
  problem,
  onDismiss,
}: {
  problem: SampleProblem | undefined;
  onDismiss: () => void;
}) {
  const text = useLocaleText();
  if (!problem) return null;
  return (
    <Notice tone="error">
      <Text>{problemText(problem, text)}</Text>
      <Action variant="quiet" onPress={onDismiss}>
        {text("ui.sample.dismiss", "Dismiss notice")}
      </Action>
    </Notice>
  );
}
