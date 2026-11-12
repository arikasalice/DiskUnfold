import type { LocaleText } from "../../i18n/presentation";
import type { HistoryEndState, HistoryRelation } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";

const states: Record<HistoryEndState, [string, string]> = {
  done: ["ui.history.state_completed", "Completed"],
  partial: ["ui.history.state_partial", "Partially completed"],
  failed: ["ui.history.state_failed", "Failed"],
  cancelled: ["ui.history.state_cancelled", "Cancelled"],
  interrupted: [
    "ui.history.state_interrupted_end_unrecorded",
    "Interrupted (end not recorded)",
  ],
};
const relations: Record<HistoryRelation, [string, string]> = {
  retry: ["ui.history.relation_retry", "Retry"],
  restore: ["ui.history.relation_restore", "Restore"],
  move_undo: ["ui.history.relation_move_undo", "Move undo"],
  effect_check: ["ui.history.relation_effect_check", "Effect check"],
};
const missing: Record<string, [string, string]> = {
  read_failed: [
    "ui.history.missing_read_failed",
    "Some records could not be read.",
  ],
  items_incomplete: [
    "ui.history.missing_items_incomplete",
    "Item results are incomplete.",
  ],
  plan_unavailable: [
    "ui.history.missing_plan_unavailable",
    "Plan information is unavailable.",
  ],
  interrupted: [
    "ui.history.missing_interrupted",
    "The operation was interrupted before its end was recorded.",
  ],
  record_failed: [
    "ui.history.missing_record_failed",
    "Some results could not be recorded.",
  ],
};

function label(text: LocaleText, value: [string, string]) {
  return text(value[0], value[1]);
}

export function kindLabel(value: string | undefined, text: LocaleText) {
  if (value === "scan.metadata")
    return text("ui.history.kind_disk_analysis", "Disk analysis");
  if (value === "duplicate.check")
    return text("ui.history.kind_duplicate_check", "Duplicate check");
  return text("ui.history.kind_unavailable", "Operation type unavailable");
}

export function stateLabel(
  value: HistoryEndState,
  compact: boolean,
  text: LocaleText,
): string {
  if (compact && value === "partial")
    return text("ui.history.state_partial_compact", "Partial");
  if (compact && value === "interrupted")
    return text("ui.history.state_interrupted_compact", "Interrupted");
  return label(text, states[value]);
}

export function stateTone(value: HistoryEndState | null | undefined) {
  if (value === "done") return "good";
  if (value === "failed" || value === "partial" || value === "interrupted")
    return "warning";
  return "neutral";
}

export function relationLabel(
  value: HistoryRelation,
  text: LocaleText,
): string {
  return label(text, relations[value]);
}

export function missingLabel(value: string, text: LocaleText): string {
  const message = missing[value] ?? [
    "ui.history.missing_generic",
    "Some information is unavailable.",
  ];
  return label(text, message);
}

export function useHistoryTime() {
  const timestamp = useTimestampText();
  const text = useLocaleText();
  return (
    value: number | null | undefined,
    unavailable = text("ui.history.time_unavailable", "Time unavailable"),
  ) => {
    if (value == null) return unavailable;
    const date = new Date(value);
    return Number.isFinite(date.getTime())
      ? timestamp(value, () => finishedTime.format(date))
      : unavailable;
  };
}

const finishedTime = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});
