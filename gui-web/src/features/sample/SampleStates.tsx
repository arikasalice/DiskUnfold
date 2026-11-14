import { Action, Notice, Progress, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { SampleProblem } from "../../state/sample-session";
import { problemText, unavailableText } from "./sample-text";

/** Progress while the sample is being prepared or reset. */
export function SampleProgress({ resetting }: { resetting: boolean }) {
  const text = useLocaleText();
  const label = resetting
    ? text("ui.sample.resetting", "Resetting the sample experience")
    : text("ui.sample.preparing", "Preparing the sample data");
  return (
    <div className="du-sample__unavailable">
      <Text>{label}</Text>
      <Progress label={label} />
    </div>
  );
}

/** Explains why the sample cannot be used; the only actions are retry and real analysis. */
export function SampleUnavailable({
  reason,
  isDisabled,
  onRetry,
  onRealAnalysis,
}: {
  reason: string | undefined;
  isDisabled: boolean;
  onRetry: (trigger: Element) => void;
  onRealAnalysis: (trigger: Element) => void;
}) {
  const text = useLocaleText();
  return (
    <section
      className="du-sample__unavailable"
      aria-label={text(
        "ui.sample.unavailable_title",
        "The sample experience is unavailable",
      )}
    >
      <Notice tone="error">
        <Text>{unavailableText(reason, text)}</Text>
      </Notice>
      <div className="du-sample__unavailable-actions">
        <Action
          isDisabled={isDisabled}
          onPress={(event) => onRetry(event.target)}
        >
          {text("ui.sample.retry", "Try again")}
        </Action>
        <Action
          variant="primary"
          isDisabled={isDisabled}
          onPress={(event) => onRealAnalysis(event.target)}
        >
          {text("ui.sample.real_analysis", "Start real analysis")}
        </Action>
      </div>
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
        {text("ui.sample.dismiss", "Dismiss")}
      </Action>
    </Notice>
  );
}
