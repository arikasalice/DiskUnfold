import type { LocaleText } from "../i18n/presentation";
import { AnalysisError } from "./analysis-error";

/*
 * The single place where scan and scope error codes and reason keys become
 * display text. Unknown codes and keys always map to a general message.
 */

/** A scan control command offered by the progress panel. */
export type ScanControl = "pause" | "resume" | "cancel";

type Message = readonly [key: string, fallback: string];

function lookup(
  table: ReadonlyMap<string, Message>,
  code: string | null | undefined,
  unknown: Message,
  text: LocaleText,
): string {
  const [key, fallback] = (code ? table.get(code) : undefined) ?? unknown;
  return text(key, fallback);
}

/** Issue-list filters in display order; "all" omits the request filter. */
export const SCAN_ISSUE_FILTERS = [
  { value: "all", key: "ui.analysis.issues_all", fallback: "All issues" },
  {
    value: "collection_failed",
    key: "ui.analysis.issues_collection_failures",
    fallback: "Collection failures",
  },
  {
    value: "unread_range",
    key: "ui.analysis.issues_unread_ranges",
    fallback: "Unread ranges",
  },
  {
    value: "uncertain",
    key: "ui.analysis.issues_uncertain_entries",
    fallback: "Uncertain entries",
  },
  {
    value: "untraversed_link",
    key: "ui.analysis.issues_untraversed_links",
    fallback: "Untraversed links",
  },
] as const;

/** A selectable issue-list filter value. */
export type ScanIssueFilter = (typeof SCAN_ISSUE_FILTERS)[number]["value"];

const issueKinds = new Map<string, Message>(
  SCAN_ISSUE_FILTERS.filter(({ value }) => value !== "all").map(
    ({ value, key, fallback }) => [value, [key, fallback]],
  ),
);
const issueStages = new Map<string, Message>([
  ["metadata", ["ui.analysis.issue_stage_metadata", "Metadata"]],
  [
    "open_directory",
    ["ui.analysis.issue_stage_open_directory", "Opening a folder"],
  ],
  ["enumerate", ["ui.analysis.issue_stage_enumerate", "Listing a folder"]],
  ["traverse", ["ui.analysis.issue_stage_traverse", "Link traversal"]],
]);
const issueReasons = new Map<string, Message>([
  [
    "scan.path_denied",
    ["ui.analysis.issue_reason_access_denied", "Access denied"],
  ],
  [
    "scan.path_vanished",
    [
      "ui.analysis.issue_reason_path_missing",
      "The item was no longer available",
    ],
  ],
  [
    "scan.unclassified",
    ["ui.analysis.issue_reason_unverified", "The item could not be verified"],
  ],
  [
    "scan.link_untraversed",
    ["ui.analysis.issue_reason_link_skipped", "The link target was skipped"],
  ],
]);

/** Names an issue kind; unknown kinds use a general label. */
export function scanIssueKindText(kind: string, text: LocaleText): string {
  return lookup(
    issueKinds,
    kind,
    ["ui.analysis.issue_unknown_kind", "Unknown issue"],
    text,
  );
}

/** Names the collector stage of an issue; unknown stages use a general label. */
export function scanIssueStageText(stage: string, text: LocaleText): string {
  return lookup(
    issueStages,
    stage,
    ["ui.analysis.issue_unknown_stage", "Unknown stage"],
    text,
  );
}

/** Explains an issue reason key; unknown keys use a general label. */
export function scanIssueReasonText(
  reasonKey: string,
  text: LocaleText,
): string {
  return lookup(
    issueReasons,
    reasonKey,
    ["ui.analysis.issue_unknown_reason", "Unknown reason"],
    text,
  );
}

/** How a read bound to one result version failed. */
export type ResultReadProblem = "stale_version" | "not_found" | "failed";

/** Classifies a result-bound read failure by its error-envelope code. */
export function resultReadProblem(failure: unknown): ResultReadProblem {
  if (failure instanceof AnalysisError) {
    if (failure.code === "STALE_VERSION") return "stale_version";
    if (failure.code === "NOT_FOUND") return "not_found";
  }
  return "failed";
}

/** Explains why the issue list for the displayed result could not be read. */
export function scanIssuesProblemText(
  problem: ResultReadProblem,
  text: LocaleText,
): string {
  switch (problem) {
    case "stale_version":
      return text(
        "ui.analysis.issues_stale",
        "These issues belong to a result that has been replaced. Reopen them with the new result.",
      );
    case "not_found":
      return text(
        "ui.analysis.issues_not_found",
        "The result for these issues is no longer available. Reopen the latest result.",
      );
    case "failed":
      return text(
        "ui.analysis.issues_failed",
        "Scan issues could not be read.",
      );
  }
}

/** Labels the recovery action offered for an issue-list problem. */
export function scanIssuesRecoveryText(
  problem: ResultReadProblem,
  text: LocaleText,
): string {
  switch (problem) {
    case "stale_version":
      return text(
        "ui.analysis.issues_reopen_new",
        "Reopen with the new result",
      );
    case "not_found":
      return text(
        "ui.analysis.issues_reopen_latest",
        "Reopen the latest result",
      );
    case "failed":
      return text("ui.analysis.issues_retry", "Retry scan issues");
  }
}

/** A scan status, totals or control problem kept by the scan state. */
export type ScanProblem =
  | { kind: "read"; problem: ResultReadProblem }
  | { kind: "totals_mismatch" }
  | { kind: "control_unavailable"; command: ScanControl }
  | { kind: "control_rejected"; command: ScanControl }
  | { kind: "already_ended" };

const controlUnavailable: Record<ScanControl, Message> = {
  pause: [
    "ui.analysis.pause_unavailable",
    "Pausing is unavailable for this scan.",
  ],
  resume: [
    "ui.analysis.resume_unavailable",
    "Resuming is unavailable for this scan.",
  ],
  cancel: [
    "ui.analysis.cancel_unavailable",
    "Cancellation is unavailable for this scan.",
  ],
};
const controlRejected: Record<ScanControl, Message> = {
  pause: [
    "ui.analysis.pause_rejected",
    "The scan could not be paused in its current state.",
  ],
  resume: [
    "ui.analysis.resume_rejected",
    "The scan could not be resumed in its current state.",
  ],
  cancel: [
    "ui.analysis.cancel_rejected",
    "The scan could not be cancelled in its current state.",
  ],
};

function readProblemText(problem: ResultReadProblem, text: LocaleText) {
  switch (problem) {
    case "stale_version":
      return text(
        "ui.analysis.result_replaced",
        "The result was replaced before it could be read. Show the latest result to continue.",
      );
    case "not_found":
      return text(
        "ui.analysis.result_not_found",
        "This scan or its result is no longer available.",
      );
    case "failed":
      return text("ui.analysis.read_failed", "Analysis could not be read.");
  }
}

/** Explains a scan state problem. */
export function scanProblemText(
  problem: ScanProblem,
  text: LocaleText,
): string {
  switch (problem.kind) {
    case "read":
      return readProblemText(problem.problem, text);
    case "totals_mismatch":
      return text(
        "ui.analysis.totals_mismatch",
        "The space totals do not match this scan's result.",
      );
    case "control_unavailable":
      return text(...controlUnavailable[problem.command]);
    case "control_rejected":
      return text(...controlRejected[problem.command]);
    case "already_ended":
      return text(
        "ui.analysis.scan_already_ended",
        "This scan has already ended.",
      );
  }
}

const scopeNotices = new Map<string, Message>([
  [
    "scope.overlap_merged",
    ["ui.analysis.scope_overlap", "Duplicate paths were combined."],
  ],
  [
    "scope.alias_merged",
    ["ui.analysis.scope_alias", "Paths to the same location were combined."],
  ],
  [
    "scope.descendant_absorbed",
    [
      "ui.analysis.scope_descendant",
      "A selected folder is already inside another selected folder.",
    ],
  ],
  [
    "scope.correspondence_unknown",
    [
      "ui.analysis.scope_uncertain",
      "Some path relationships could not be confirmed.",
    ],
  ],
  [
    "scope.excluded",
    [
      "ui.analysis.scope_excluded",
      "An exclusion removed a selected folder from the scan.",
    ],
  ],
  [
    "scope.exclusion_ineffective",
    [
      "ui.analysis.scope_exclusion_outside",
      "An exclusion falls outside the selected folders.",
    ],
  ],
  [
    "scan.path_vanished",
    [
      "ui.analysis.scope_missing",
      "A selected folder no longer exists. Check the path and review the scope again.",
    ],
  ],
  [
    "scan.path_denied",
    [
      "ui.analysis.scope_denied",
      "Access to a selected folder was denied. Check its permissions and review the scope again.",
    ],
  ],
  [
    "scan.disconnected",
    [
      "ui.analysis.scope_disconnected",
      "A selected drive was disconnected. Reconnect it and review the scope again.",
    ],
  ],
  [
    "path.unresolvable",
    [
      "ui.analysis.scope_unresolvable",
      "A selected folder path could not be resolved. Check the path and review the scope again.",
    ],
  ],
]);

/** Explains a scope review notice or target reason; unknown keys ask for review. */
export function scopeNoticeText(
  key: string,
  text: LocaleText = (messageKey, fallback) => fallback || messageKey,
): string {
  return lookup(
    scopeNotices,
    key,
    [
      "ui.analysis.scope_needs_review",
      "A selected path needs attention. Check the folder paths and review the scope again.",
    ],
    text,
  );
}

const jobReasons = new Map<string, Message>([
  [
    "job.cancelled",
    ["ui.analysis.scan_reason_cancelled", "The scan was cancelled."],
  ],
  [
    "job.shutting_down",
    [
      "ui.analysis.scan_reason_shutting_down",
      "The scan stopped because DiskUnfold is closing.",
    ],
  ],
  [
    "job.queue_full",
    [
      "ui.analysis.scan_reason_queue_full",
      "Too many jobs are waiting. Try the scan again later.",
    ],
  ],
]);

/** Explains the reason key reported with a scan job status. */
export function scanJobReasonText(reasonKey: string, text: LocaleText): string {
  if (scopeNotices.has(reasonKey)) return scopeNoticeText(reasonKey, text);
  return lookup(
    jobReasons,
    reasonKey,
    ["ui.analysis.scan_reason_unknown", "The scan reported a problem."],
    text,
  );
}

/** Explains a scope review or scan start failure. */
export function scopeErrorText(error: unknown, text: LocaleText): string {
  if (error instanceof AnalysisError) {
    if (error.code === "STALE_VERSION")
      return text(
        "ui.analysis.target_or_result_changed",
        "The target or result changed. Review the scope again.",
      );
    if (scopeNotices.has(error.reasonKey))
      return scopeNoticeText(error.reasonKey, text);
  }
  return text("ui.analysis.operation_failed", "The operation failed");
}

const reviewReasons = new Map<string, Message>([
  [
    "path.unresolvable",
    [
      "ui.analysis.review_valid_path",
      "Enter a valid local folder path, then review the scope again.",
    ],
  ],
  [
    "scope.target_unavailable",
    [
      "ui.analysis.review_accessible_paths",
      "Check that every selected path is an accessible local folder, then review the scope again.",
    ],
  ],
]);

/** Explains a failed scope review request. */
export function scopeReviewErrorText(error: unknown, text: LocaleText): string {
  const message =
    error instanceof AnalysisError
      ? reviewReasons.get(error.reasonKey)
      : undefined;
  return message ? text(...message) : scopeErrorText(error, text);
}

/** The target evidence a rejected scope review reports. */
type ReviewedTargets = {
  readonly targets: readonly {
    readonly access: string;
    readonly reasonKey: string | null;
  }[];
};

/** Explains why a completed scope review cannot start a scan. */
export function rejectedScopeReviewText(
  review: ReviewedTargets,
  text: LocaleText,
): string {
  const reason = review.targets.find(
    (target) => target.access !== "accessible" && target.reasonKey,
  )?.reasonKey;
  const message = reason ? reviewReasons.get(reason) : undefined;
  if (message) return text(...message);
  if (reason) return scopeNoticeText(reason, text);
  if (review.targets.some((target) => target.access === "denied"))
    return scopeNoticeText("scan.path_denied", text);
  return text(
    "ui.analysis.review_unverified_paths",
    "One or more selected paths could not be verified. Review all paths before starting.",
  );
}
