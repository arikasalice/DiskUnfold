import { LiveStatus, StatusBadge, Text } from "../../components";
import type { StatusRow, StatusTone } from "../../components";
import type {
  ModelResponse,
  VisualizationMessageKey,
  VizText,
} from "../../viz";
import { visualizationReason } from "../../viz";

const UNWATCHED = "viz.staleness.unwatched";
const CURRENT_ORIGINS: ReadonlySet<string> = new Set([
  "scan",
  "current_analysis",
]);

type Badge = { key: string; tone: StatusTone; label: string };

/**
 * A neutral badge would draw the plain circle of the neutral tone, which reads
 * as a radio button. It takes the information shape instead.
 */
const NEUTRAL_NOTE: StatusRow = {
  id: "viz-status-note",
  tone: "neutral",
  shape: "info",
  messageKey: "viz-status-note",
  colorToken: "--du-muted",
};

/**
 * Short badge words for the completeness values that need attention. The
 * evidence table names every value with the longer result-version words;
 * a complete result has no badge.
 */
const COMPLETENESS_BADGES: Readonly<
  Record<string, { tone: StatusTone; label: VisualizationMessageKey }>
> = {
  partial: { tone: "info", label: "badgePartial" },
  missing: { tone: "warning", label: "badgeMissing" },
  cancelled: { tone: "neutral", label: "badgeCancelled" },
};

/**
 * The status line of one model. A badge appears only for a fact that changes
 * what the values mean: an unfinished or incomplete result, a result that is
 * not from the current analysis, or one that changed after the analysis.
 * A complete, current result has no badge. Results that are not watched for
 * later changes, which is every result today, say so in one quiet sentence
 * next to the observation time, except while a scan is still running.
 * Badges sit in one status region so a screen reader announces a new badge once;
 * the observation time and the sentence stay outside it.
 */
export function ModelStatus({
  frame,
  text,
}: {
  frame: ModelResponse;
  text: VizText;
}) {
  const { completeness, model } = frame;
  const { staleness } = model;
  const badges: Badge[] = [];
  const completenessBadge = COMPLETENESS_BADGES[completeness];
  if (completenessBadge)
    badges.push({
      key: "completeness",
      tone: completenessBadge.tone,
      label: text.message(completenessBadge.label),
    });
  if (!CURRENT_ORIGINS.has(model.origin))
    badges.push({
      key: "origin",
      tone: "neutral",
      label: text.key(model.origin),
    });
  const unwatched =
    staleness.state === "unknown" && staleness.reasonKey === UNWATCHED;
  if (staleness.state === "stale")
    badges.push({
      key: "freshness",
      tone: "warning",
      label: text.key("stale"),
    });
  else if (staleness.state !== "current" && !unwatched)
    badges.push({
      key: "freshness",
      tone: "neutral",
      label: text.key("unknown"),
    });
  const reason =
    staleness.state !== "current" && staleness.state !== "stale" && !unwatched
      ? staleness.reasonKey
      : null;
  const scanRunning = completeness === "partial";
  return (
    <>
      <LiveStatus>
        {badges.map(({ key, tone, label }) => (
          <StatusBadge
            key={key}
            tone={tone}
            {...(tone === "neutral" ? { status: NEUTRAL_NOTE } : {})}
          >
            {label}
          </StatusBadge>
        ))}
      </LiveStatus>
      <Text>{text.observedAt(frame.observedAt)}</Text>
      <Text>{scopeTotal(model, text)}</Text>
      {unwatched && !scanRunning && (
        <Text kind="muted">{visualizationReason(UNWATCHED, text.message)}</Text>
      )}
      {reason !== null && (
        <Text kind="muted">{visualizationReason(reason, text.message)}</Text>
      )}
    </>
  );
}

/**
 * The scope total, with the size basis when it is a size: the unique physical
 * size differs from what a file manager shows (F06 README 36).
 */
function scopeTotal(model: ModelResponse["model"], text: VizText) {
  const value = text.quantity(model.totals.scope.quantity);
  return model.totals.scope.quantity.unit === "bytes"
    ? text.message("scopeTotalBasisLine", {
        value,
        basis: text.key(model.representation.sizeBasis),
      })
    : text.message("scopeTotalLine", { value });
}
