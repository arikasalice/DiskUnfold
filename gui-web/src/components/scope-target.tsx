import { useLocaleText } from "../i18n/presentation";
import { Text } from "./presentation";
import { MiddleEllipsisText } from "./text-fit";
import "../theme/scope-target.css";

/** Shows the first recorded scope root and the number of additional roots. */
export function ScopeTarget({
  firstRoot,
  rootCount,
  fallback,
}: {
  /** First recorded root; its middle is ellipsized, with the full path on hover. */
  firstRoot?: string | undefined;
  /** Number of roots represented by firstRoot. */
  rootCount?: number | undefined;
  /** Summary used when no first root was recorded. */
  fallback?: string | undefined;
}) {
  const text = useLocaleText();
  const root = firstRoot || fallback;
  if (!root) return null;
  const more = firstRoot ? Math.max(0, (rootCount ?? 1) - 1) : 0;

  return (
    <div className="du-scope-target">
      <MiddleEllipsisText text={root} cell />
      {more > 0 && (
        <Text kind="muted">
          {text("ui.history.roots_more", "and {count} more", {
            count: BigInt(more),
          })}
        </Text>
      )}
    </div>
  );
}
