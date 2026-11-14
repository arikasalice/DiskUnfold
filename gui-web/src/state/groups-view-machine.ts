import type { Id, ItemRef } from "../ipc";

/** Phases of the virtual-group view (FR2.HD1.gui-web.DD1). */
export type GroupViewPhase =
  | "idle"
  | "requesting"
  | "working"
  | "showing"
  | "updatable"
  | "requery_needed"
  | "not_applicable";

/** Inputs that may move the view between phases. */
export type GroupViewEvent =
  /** The user asked for a classification (also a retry or an update). */
  | "request"
  /** The classify call answered with a summary. */
  | "summary"
  /** The classify call answered with a background job. */
  | "job"
  /** The classify call answered that the source cannot be grouped. */
  | "not_applicable"
  /** The classify call failed without a stale-result reason. */
  | "request_failed"
  /** The job finished as done and its summary was read. */
  | "job_summary"
  /** The job was cancelled or failed; the previous result stays. */
  | "job_ended"
  /** results.version_changed announced a newer result. */
  | "version_changed"
  /** A reason key or a mismatched answer made the shown result unusable. */
  | "stale"
  /** The rules changed after the shown classification. */
  | "rules_changed";

type Transitions = Readonly<
  Record<
    GroupViewPhase,
    Readonly<Partial<Record<GroupViewEvent, GroupViewPhase>>>
  >
>;

/** The complete transition table. An event absent from a row is ignored. */
export const GROUP_VIEW_TRANSITIONS: Transitions = {
  idle: { request: "requesting" },
  requesting: {
    summary: "showing",
    job: "working",
    not_applicable: "not_applicable",
    request_failed: "showing",
    stale: "requery_needed",
  },
  working: {
    job_summary: "showing",
    job_ended: "showing",
    stale: "requery_needed",
  },
  showing: {
    request: "requesting",
    version_changed: "updatable",
    stale: "requery_needed",
    rules_changed: "requery_needed",
  },
  updatable: {
    request: "requesting",
    stale: "requery_needed",
    rules_changed: "requery_needed",
  },
  requery_needed: { request: "requesting" },
  not_applicable: { request: "requesting" },
};

/** Returns the next phase, or undefined when the event does not apply. */
export function nextGroupViewPhase(
  phase: GroupViewPhase,
  event: GroupViewEvent,
): GroupViewPhase | undefined {
  return GROUP_VIEW_TRANSITIONS[phase][event];
}

const REQUERY_REASONS: ReadonlySet<string> = new Set([
  "result.version_stale",
  "result.requery_required",
  "group.result_expired",
  "group.rules_modified",
]);

/** True when a reason key means the pinned result or rules must be requested again. */
export function isRequeryReason(reasonKey: string | undefined): boolean {
  return reasonKey !== undefined && REQUERY_REASONS.has(reasonKey);
}

/** The conditions a response must still match when it arrives. */
export type ViewCondition = {
  resultVersion: Id | undefined;
  scope: ItemRef | undefined;
  rulesetVersion: number;
  rulesetFingerprint: string | undefined;
};

/** What a response or finished job states about itself. */
export type ResponseFacts = {
  requestSeq: number;
  resultVersion?: Id | undefined;
  rulesetVersion?: number | undefined;
  rulesetFingerprint?: string | undefined;
};

/** The outcome of judging a response against its channel and current conditions. */
export type Verdict = "accept" | "superseded" | "mismatch";

/**
 * One view channel. The view reference names the screen; the sequence only
 * grows, so a newer request makes every older answer recognizable.
 */
export class ViewChannel {
  private seq = 0;

  constructor(readonly viewRef: Id) {}

  /** The most recently issued request sequence (zero before the first). */
  get last(): number {
    return this.seq;
  }

  /** Issues the next sequence; sequences are never reused. */
  next(): number {
    this.seq += 1;
    return this.seq;
  }
}

/**
 * Decides whether a response may be shown. In order: (1) its sequence is the
 * channel's latest, (2) its result version is the current one, (3) its rule
 * set version and fingerprint are the current ones. Anything else is dropped.
 */
export function judgeResponse(
  channel: ViewChannel,
  current: ViewCondition,
  facts: ResponseFacts,
): Verdict {
  if (facts.requestSeq !== channel.last) return "superseded";
  if (
    current.resultVersion !== undefined &&
    facts.resultVersion !== undefined &&
    facts.resultVersion !== current.resultVersion
  )
    return "mismatch";
  if (
    facts.rulesetVersion !== undefined &&
    facts.rulesetVersion !== current.rulesetVersion
  )
    return "mismatch";
  if (
    current.rulesetFingerprint !== undefined &&
    facts.rulesetFingerprint !== undefined &&
    facts.rulesetFingerprint !== current.rulesetFingerprint
  )
    return "mismatch";
  return "accept";
}

/** Compares two item references by result version and item number. */
export function sameItemRef(
  left: ItemRef | undefined,
  right: ItemRef | undefined,
): boolean {
  return (
    left === right ||
    (left !== undefined &&
      right !== undefined &&
      left.resultVersion === right.resultVersion &&
      left.itemNo === right.itemNo)
  );
}
