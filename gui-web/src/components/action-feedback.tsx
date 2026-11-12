import { Action } from "./controls";
import { useLocaleText } from "../i18n/presentation";
import { LiveStatus } from "./presentation";

/** Persistent feedback occupies its own layout row and keeps full names accessible. */
export function ActionFeedback({
  status,
  parts,
  pending,
  onDismiss,
}: {
  status: string;
  parts: { beforeName: string; name: string; afterName: string } | undefined;
  pending: boolean;
  onDismiss: (trigger: Element) => void;
}) {
  const text = useLocaleText();
  return (
    <div className="du-browse-feedback">
      <LiveStatus>
        {parts ? (
          <>
            {parts.beforeName}
            <span className="du-feedback-name">{parts.name}</span>
            {parts.afterName}
          </>
        ) : (
          status
        )}
      </LiveStatus>
      {status && !pending && (
        <Action onPress={(event) => onDismiss(event.target)}>
          {text("ui.common.dismiss", "Dismiss")}
        </Action>
      )}
    </div>
  );
}
