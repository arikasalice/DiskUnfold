import type { HistoryEndState, HistoryRelation } from "../../ipc";
import { useTimestampText } from "../../i18n/regional-format";

const states: Record<HistoryEndState, string> = {
  done: "Completed",
  partial: "Partially completed",
  failed: "Failed",
  cancelled: "Cancelled",
  interrupted: "Interrupted (end not recorded)",
};
const relations: Record<HistoryRelation, string> = {
  retry: "Retry",
  restore: "Restore",
  move_undo: "Move undo",
  effect_check: "Effect check",
};
const missing: Record<string, string> = {
  read_failed: "Some records could not be read.",
  items_incomplete: "Item results are incomplete.",
  plan_unavailable: "Plan information is unavailable.",
  interrupted: "The operation was interrupted before its end was recorded.",
  record_failed: "Some results could not be recorded.",
};
const kinds: Record<string, string> = {
  "scan.metadata": "Disk analysis",
  "duplicate.check": "Duplicate check",
};

export function kindLabel(value?: string) {
  return value && Object.hasOwn(kinds, value)
    ? kinds[value]
    : "Operation type unavailable";
}

export function stateLabel(value: HistoryEndState, compact = false): string {
  if (compact && value === "partial") return "Partial";
  if (compact && value === "interrupted") return "Interrupted";
  return Object.hasOwn(states, value) ? states[value] : "State unavailable";
}

export function stateTone(value: HistoryEndState | null | undefined) {
  if (value === "done") return "good";
  if (value === "failed" || value === "partial" || value === "interrupted")
    return "warning";
  return "neutral";
}

export function relationLabel(value: HistoryRelation): string {
  return Object.hasOwn(relations, value)
    ? relations[value]
    : "Relation unavailable";
}

export function missingLabel(value: string): string {
  return (
    (Object.hasOwn(missing, value) ? missing[value] : undefined) ??
    "Some information is unavailable."
  );
}

const finishedTime = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

export function useHistoryTime() {
  const timestamp = useTimestampText();
  return (
    value: number | null | undefined,
    unavailable = "Time unavailable",
  ) => {
    if (value == null) return unavailable;
    const date = new Date(value);
    return Number.isFinite(date.getTime())
      ? timestamp(value, () => finishedTime.format(date))
      : unavailable;
  };
}
