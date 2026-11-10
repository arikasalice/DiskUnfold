import type { HistoryEndState, HistoryRelation } from "../../ipc";

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

export function stateLabel(value: HistoryEndState): string {
  return Object.hasOwn(states, value) ? states[value] : "State unavailable";
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
