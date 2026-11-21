import { useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import type {
  AnalysisClient,
  HistoryRetentionPolicy,
  HistoryRetentionReview,
  HistoryRetentionReviewRequest,
} from "../../ipc";
import {
  errorKey,
  isReason,
  needsConfirmation,
  parseDays,
} from "./history-retention-model";
import type {
  ActiveReview,
  RetentionPanelError,
  ReviewIntent,
  ReviewSource,
} from "./history-retention-model";

type ReviewDependencies = {
  policy: HistoryRetentionPolicy | undefined;
  draftDays: string;
  setPolicy: Dispatch<SetStateAction<HistoryRetentionPolicy | undefined>>;
  setDraftDays: Dispatch<SetStateAction<string>>;
  setPanelError: Dispatch<SetStateAction<RetentionPanelError | undefined>>;
  inputRef: { current: HTMLInputElement | null };
  mounted: { current: boolean };
};

/** Owns the retention review, apply and cancellation lifecycle. */
export function useHistoryRetentionReview(
  client: AnalysisClient,
  {
    policy,
    draftDays,
    setPolicy,
    setDraftDays,
    setPanelError,
    inputRef,
    mounted,
  }: ReviewDependencies,
) {
  const [busy, setBusy] = useState(false);
  const [activeReview, setActiveReview] = useState<ActiveReview>();
  const [announcement, setAnnouncement] = useState<{
    key: "enabled" | "disabled" | "period_saved";
    days?: number;
    sequence: number;
  }>();
  const [validationAttempted, setValidationAttempted] = useState(false);
  const [suggestionChanged, setSuggestionChanged] = useState(false);
  const nextDialogGeneration = useRef(0);
  const cancelledReviews = useRef(new Set<string>());
  const resolvedReviews = useRef(new Set<string>());

  function cancelReview(reviewRef: string) {
    if (
      resolvedReviews.current.has(reviewRef) ||
      cancelledReviews.current.has(reviewRef)
    )
      return;
    cancelledReviews.current.add(reviewRef);
    void client
      .call("history.retention_cancel", { reviewRef })
      .catch(() => undefined);
  }

  async function refreshPolicy(
    preserveDraft: boolean,
    fallback?: HistoryRetentionPolicy,
  ) {
    try {
      const next = await client.call("history.retention_get", {});
      if (mounted.current) {
        setPolicy(next);
        setPanelError(undefined);
        if (!preserveDraft) {
          setDraftDays(String(next.specifiedDays ?? next.suggestedDays));
          setSuggestionChanged(false);
          setValidationAttempted(false);
        }
      }
      return next;
    } catch {
      if (mounted.current) {
        if (fallback) setPolicy(fallback);
        setPanelError("load_failed");
      }
      return fallback;
    }
  }

  function announceApplied(
    current: HistoryRetentionPolicy,
    intent: ReviewIntent,
  ) {
    setAnnouncement((previous) => {
      const sequence = (previous?.sequence ?? 0) + 1;
      if (intent.source === "period")
        return {
          key: "period_saved",
          days: current.specifiedDays ?? current.suggestedDays,
          sequence,
        };
      return {
        key: current.enabled ? "enabled" : "disabled",
        sequence,
      };
    });
  }

  async function applyReviewed(
    review: HistoryRetentionReview,
    intent: ReviewIntent,
    fromDialog: boolean,
    staleRetries: number,
  ) {
    try {
      const applied = await client.call("history.retention_apply", {
        reviewRef: review.reviewRef,
        fingerprint: review.fingerprint,
      });
      if (!mounted.current) return;
      resolvedReviews.current.add(review.reviewRef);
      setActiveReview(undefined);
      setPolicy(applied.policy);
      const refreshed = await refreshPolicy(
        intent.source === "switch",
        applied.policy,
      );
      announceApplied(refreshed ?? applied.policy, intent);
    } catch (error) {
      if (isReason(error, "history.review_stale") && staleRetries === 0) {
        try {
          const fresh = await client.call(
            "history.retention_review",
            intent.request,
          );
          if (fresh.confirmationRequired !== needsConfirmation(fresh)) {
            cancelReview(fresh.reviewRef);
            cancelReview(review.reviewRef);
            setActiveReview(undefined);
            setPanelError("change_failed");
            return;
          }
          cancelReview(review.reviewRef);
          if (fromDialog || needsConfirmation(fresh)) {
            setActiveReview({
              review: fresh,
              intent,
              stale: true,
              staleRetries: 1,
              generation: ++nextDialogGeneration.current,
            });
          } else {
            await applyReviewed(fresh, intent, false, 1);
          }
        } catch (reviewError) {
          cancelReview(review.reviewRef);
          setActiveReview(undefined);
          setPanelError(errorKey(reviewError));
        }
        return;
      }
      if (isReason(error, "history.count_unavailable")) {
        cancelReview(review.reviewRef);
        setActiveReview(undefined);
        setPanelError("count_unavailable");
        return;
      }
      if (fromDialog) {
        setActiveReview((current) =>
          current
            ? {
                ...current,
                dialogError: isReason(error, "history.policy_store_failed")
                  ? "save_failed"
                  : "change_failed",
              }
            : current,
        );
      } else {
        setPanelError("change_failed");
      }
    }
  }

  async function reviewChange(
    request: HistoryRetentionReviewRequest,
    source: ReviewSource,
  ) {
    setBusy(true);
    setPanelError(undefined);
    setValidationAttempted(false);
    try {
      const review = await client.call("history.retention_review", request);
      if (!mounted.current) return;
      if (review.confirmationRequired !== needsConfirmation(review)) {
        cancelReview(review.reviewRef);
        setPanelError("change_failed");
        return;
      }
      const intent = { request, source };
      if (needsConfirmation(review)) {
        setActiveReview({
          review,
          intent,
          stale: false,
          staleRetries: 0,
          generation: ++nextDialogGeneration.current,
        });
      } else {
        await applyReviewed(review, intent, false, 0);
      }
    } catch (error) {
      setPanelError(errorKey(error));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  function changeEnabled(enabled: boolean) {
    if (!policy) return;
    const days = enabled ? parseDays(draftDays) : undefined;
    if (enabled && days === undefined) {
      setValidationAttempted(true);
      inputRef.current?.focus();
      return;
    }
    void reviewChange(
      { enabled, ...(days !== undefined && { days }) },
      "switch",
    );
  }

  function applyPeriod() {
    if (!policy) return;
    const days = parseDays(draftDays);
    if (days === undefined) {
      setValidationAttempted(true);
      inputRef.current?.focus();
      return;
    }
    void reviewChange({ enabled: policy.enabled, days }, "period");
  }

  async function confirmReview() {
    if (!activeReview || busy) return;
    setBusy(true);
    setActiveReview((current) =>
      current ? { ...current, dialogError: undefined } : current,
    );
    try {
      await applyReviewed(
        activeReview.review,
        activeReview.intent,
        true,
        activeReview.staleRetries,
      );
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  function cancelActiveReview() {
    if (!activeReview) return;
    cancelReview(activeReview.review.reviewRef);
    setActiveReview(undefined);
  }

  function updateDraftDays(value: string) {
    setDraftDays(value);
    if (
      policy?.specifiedDays === undefined &&
      value !== String(policy?.suggestedDays)
    )
      setSuggestionChanged(true);
  }

  return {
    busy,
    activeReview,
    announcement,
    validationAttempted,
    suggestionChanged,
    setDraftDays: updateDraftDays,
    changeEnabled,
    applyPeriod,
    confirmReview,
    cancelActiveReview,
  };
}
