import { AnalysisError } from "../../ipc";
import type {
  HistoryRetentionReview,
  HistoryRetentionReviewRequest,
} from "../../ipc";

export type ReviewSource = "switch" | "period";
export type ReviewIntent = {
  request: HistoryRetentionReviewRequest;
  source: ReviewSource;
};
export type ActiveReview = {
  review: HistoryRetentionReview;
  intent: ReviewIntent;
  stale: boolean;
  staleRetries: number;
  dialogError?: "save_failed" | "change_failed" | undefined;
  generation: number;
};
export type RetentionPanelError =
  "load_failed" | "count_unavailable" | "change_failed";

export function parseDays(value: string) {
  if (!/^\d+$/.test(value)) return undefined;
  const days = Number(value);
  return Number.isSafeInteger(days) && days >= 1 ? days : undefined;
}

export function needsConfirmation(review: HistoryRetentionReview) {
  return review.changeKind === "enable" || review.changeKind === "shorten";
}

export function errorKey(
  error: unknown,
): "count_unavailable" | "change_failed" {
  return error instanceof AnalysisError &&
    error.reasonKey === "history.count_unavailable"
    ? "count_unavailable"
    : "change_failed";
}

export function isReason(error: unknown, reason: string) {
  return error instanceof AnalysisError && error.reasonKey === reason;
}
