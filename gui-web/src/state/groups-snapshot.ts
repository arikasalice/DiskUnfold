import type {
  EventEnvelope,
  GroupRuleset,
  GroupSummary,
  Id,
  ItemRef,
} from "../ipc";
import type { GroupViewPhase } from "./groups-view-machine";

/** How a classification ended without a new result. */
export type GroupsOutcome =
  | { kind: "cancelled" }
  | { kind: "failed"; reasonKey?: string | undefined; retryable: boolean };

/** Why the shown result can no longer be trusted. */
export type GroupsStaleReason = "version" | "expired" | "rules";

/** What the view classifies: a pinned result and an optional folder inside it. */
export type GroupsTarget = {
  resultVersion?: Id | undefined;
  scope?: ItemRef | undefined;
};

/** Immutable view model published by the groups controller. */
export type GroupsSnapshot = {
  phase: GroupViewPhase;
  ruleset: GroupRuleset | undefined;
  rulesStatus: "loading" | "ready" | "error";
  /** The last accepted classification; kept while a newer one is awaited. */
  summary: GroupSummary | undefined;
  outcome: GroupsOutcome | undefined;
  /** Reason key of a failed request or of an inapplicable source. */
  reasonKey: string | undefined;
  staleReason: GroupsStaleReason | undefined;
  /** A newer result version, or "unknown" when the event did not name it. */
  newerResult: Id | "unknown" | undefined;
  /** The saved rules differ from the rules of the shown classification. */
  rulesChanged: boolean;
  jobRef: Id | undefined;
  scope: ItemRef | undefined;
};

/** True while the visible summary is not the answer to the latest request. */
export function showsPreviousClassification(snapshot: GroupsSnapshot) {
  if (!snapshot.summary) return false;
  return (
    snapshot.phase === "requesting" ||
    snapshot.phase === "working" ||
    snapshot.phase === "requery_needed" ||
    snapshot.phase === "not_applicable" ||
    snapshot.outcome !== undefined
  );
}

/** The result version an event names in its envelope or body, if any. */
export function eventVersion(event: EventEnvelope): Id | undefined {
  if (typeof event.resultVersion === "string") return event.resultVersion;
  const body = event.body;
  if (body !== null && typeof body === "object" && "resultVersion" in body)
    return typeof body.resultVersion === "string"
      ? body.resultVersion
      : undefined;
  return undefined;
}
