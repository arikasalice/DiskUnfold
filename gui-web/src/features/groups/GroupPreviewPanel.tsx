import { useSyncExternalStore } from "react";
import { Action, LiveStatus, Metric, Notice } from "../../components";
import type { SizeDisplay } from "../../i18n/bytes";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type {
  GroupSummary,
  GroupSummaryRow,
  GroupTotal,
  GroupTotals,
} from "../../ipc";
import type { DraftRule } from "../../state/groups-conditions";
import type { ReadState, TotalsKey } from "../../state/groups-details";
import type {
  GroupPreview,
  PreviewResult,
  PreviewSnapshot,
} from "../../state/groups-preview";
import { countText, rowCountTexts, totalSizeText } from "./group-format";
import { reasonLabel } from "./group-text";

type Shown = {
  row: GroupSummaryRow;
  total: GroupTotal | undefined;
  /** The size is still being read. */
  loading: boolean;
};

function fromPreview(result: PreviewResult, key: string): Shown | undefined {
  const row = result.summary.groups[result.keys.indexOf(key)];
  if (!row) return undefined;
  return {
    row,
    total: result.totals?.groups.find((entry) => entry.groupId === row.groupId)
      ?.total,
    loading: false,
  };
}

/** The applied result of a stored group stands in until a preview is calculated. */
function fromApplied(
  summary: GroupSummary | undefined,
  totals: ReadState<TotalsKey, GroupTotals>,
  groupId: string | undefined,
): Shown | undefined {
  const row = summary?.groups.find((entry) => entry.groupId === groupId);
  if (!row) return undefined;
  return {
    row,
    total:
      totals.phase === "ready"
        ? totals.value.groups.find((entry) => entry.groupId === row.groupId)
            ?.total
        : undefined,
    loading: totals.phase === "idle" || totals.phase === "loading",
  };
}

function shownOf(
  state: PreviewSnapshot,
  rule: DraftRule | undefined,
  summary: GroupSummary | undefined,
  totals: ReadState<TotalsKey, GroupTotals>,
): Shown | undefined {
  if (!rule || state.phase === "failed" || state.phase === "not_applicable")
    return undefined;
  const computed = state.result && fromPreview(state.result, rule.key);
  return computed ?? fromApplied(summary, totals, rule.groupId);
}

function note(state: PreviewSnapshot, hasValues: boolean, text: LocaleText) {
  const earlier = hasValues
    ? ` ${text("ui.groups.preview_earlier", "The values shown are for the earlier conditions.")}`
    : "";
  if (state.phase === "waiting" || state.phase === "computing")
    return `${text("ui.groups.preview_computing", "Recalculating…")}${earlier}`;
  if (state.phase === "blocked")
    return `${text("ui.groups.preview_blocked", "Fix the marked inputs to recalculate.")}${earlier}`;
  return text(
    "ui.groups.preview_hint",
    "When you change a condition, the preview is recalculated within 2 seconds.",
  );
}

/**
 * The preview of the group being edited: its member count, size, overlap and
 * undecidable count for the draft rules. Nothing here is saved.
 */
export function GroupPreviewPanel({
  preview,
  rule,
  summary,
  totals,
  display,
}: {
  preview: GroupPreview;
  rule: DraftRule | undefined;
  summary: GroupSummary | undefined;
  totals: ReadState<TotalsKey, GroupTotals>;
  display: SizeDisplay;
}) {
  const text = useLocaleText();
  const state = useSyncExternalStore(preview.subscribe, preview.getSnapshot);
  const shown = shownOf(state, rule, summary, totals);
  const title = text("ui.groups.preview_title", "Preview");
  const counts = shown && rowCountTexts(shown.row, text);
  return (
    <section className="du-groups-preview" aria-label={title}>
      <Metric
        label={title}
        prominent
        detail={
          shown && counts
            ? text(
                "ui.groups.preview_overlap",
                "In several groups {overlap} \u00b7 Cannot decide {undecided}",
                {
                  overlap: counts.overlap,
                  undecided: countText(shown.row.undecidableCount, false, text),
                },
              )
            : undefined
        }
      >
        {shown && counts
          ? text(
              "ui.groups.preview_members",
              "Members {members} \u00b7 {size}",
              {
                members: counts.members,
                size: shown.loading
                  ? text("ui.browse.loading", "Loading…")
                  : totalSizeText(shown.total, display, text),
              },
            )
          : text("ui.groups.preview_none", "No preview yet")}
      </Metric>
      {state.phase === "failed" || state.phase === "not_applicable" ? (
        <Notice tone={state.phase === "failed" ? "error" : "info"}>
          {state.phase === "failed"
            ? text(
                "ui.groups.preview_failed",
                "The preview could not be calculated.",
              )
            : text(
                "ui.groups.preview_not_applicable",
                "This result cannot be previewed.",
              )}{" "}
          {reasonLabel(state.reasonKey, text)}{" "}
          {state.phase === "failed" && (
            <Action onPress={() => preview.retry()}>
              {text("ui.groups.retry", "Try again")}
            </Action>
          )}
        </Notice>
      ) : (
        <LiveStatus>{note(state, shown !== undefined, text)}</LiveStatus>
      )}
    </section>
  );
}
