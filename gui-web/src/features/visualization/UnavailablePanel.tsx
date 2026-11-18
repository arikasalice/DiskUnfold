import { useId } from "react";
import { Action, Heading, LiveStatus, Text } from "../../components";
import type { VizText } from "../../viz";
import { visualizationUnavailableText } from "../../viz";

/** A way out of the view, offered next to the notice. */
export type BackAction = { label: string; onPress: () => void };

/**
 * Takes the place of the chart when the engine cannot draw the requested
 * format. It states what is missing and what to do, draws no chart, and takes
 * no focus. Its text is a status region, so it is announced once when it
 * appears; `backAction` leaves the view from beside the text.
 */
export function UnavailablePanel({
  reasonKey,
  text,
  backAction,
}: {
  reasonKey: string;
  text: VizText;
  backAction?: BackAction | undefined;
}) {
  const headingId = useId();
  const { title, body } = visualizationUnavailableText(reasonKey, text.message);
  return (
    <section className="du-review-stack" aria-labelledby={headingId}>
      <LiveStatus>
        <Heading level={3} id={headingId}>
          {title}
        </Heading>
        <Text>{body}</Text>
      </LiveStatus>
      {backAction && (
        <div className="du-review-actions">
          <Action onPress={backAction.onPress}>{backAction.label}</Action>
        </div>
      )}
    </section>
  );
}
