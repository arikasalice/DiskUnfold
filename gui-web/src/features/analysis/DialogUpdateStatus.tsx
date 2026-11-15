import { LiveStatus, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";

/**
 * The one line a result dialog shows after it moved to the newest result.
 * The status region stays mounted so the line is announced when it appears.
 */
export function DialogUpdateStatus({ updated }: { updated: boolean }) {
  const text = useLocaleText();
  return (
    <LiveStatus>
      {updated && (
        <Text>
          {text(
            "ui.analysis.updated_to_latest",
            "Updated to the latest result",
          )}
        </Text>
      )}
    </LiveStatus>
  );
}
