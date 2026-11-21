import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  Action,
  ConfirmDialog,
  Heading,
  IntegerField,
  LiveStatus,
  Notice,
  Progress,
  Readout,
  StatusBadge,
  SwitchField,
  Text,
  isImeProcessingKey,
} from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  HistoryRetentionPolicy,
  HistoryRetentionReview,
  HistoryRetentionReviewRequest,
} from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import "../../theme/history-retention.css";

type ReviewSource = "switch" | "period";
type ReviewIntent = {
  request: HistoryRetentionReviewRequest;
  source: ReviewSource;
};
type ActiveReview = {
  review: HistoryRetentionReview;
  intent: ReviewIntent;
  stale: boolean;
  staleRetries: number;
  dialogError?: "save_failed" | "change_failed" | undefined;
  generation: number;
};

function parseDays(value: string) {
  if (!/^\d+$/.test(value)) return undefined;
  const days = Number(value);
  return Number.isSafeInteger(days) && days >= 1 ? days : undefined;
}

function needsConfirmation(review: HistoryRetentionReview) {
  return review.changeKind === "enable" || review.changeKind === "shorten";
}

function errorKey(error: unknown): "count_unavailable" | "change_failed" {
  return error instanceof AnalysisError &&
    error.reasonKey === "history.count_unavailable"
    ? "count_unavailable"
    : "change_failed";
}

function isReason(error: unknown, reason: string) {
  return error instanceof AnalysisError && error.reasonKey === reason;
}

function localizedError(
  key: "count_unavailable" | "change_failed",
  text: ReturnType<typeof useLocaleText>,
) {
  return key === "count_unavailable"
    ? text(
        "ui.history.retention.count_unavailable",
        "History to be removed couldn't be counted, so this can't be changed. Try again shortly.",
      )
    : text(
        "ui.history.retention.change_failed",
        "The retention setting couldn't be changed. Try again.",
      );
}

/** Owns the retention controls' review, apply and cancellation lifecycle. */
export function useHistoryRetention(client: AnalysisClient) {
  const [policy, setPolicy] = useState<HistoryRetentionPolicy>();
  const [draftDays, setDraftDays] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [isOpen, setIsOpen] = useState(false);
  const [activeReview, setActiveReview] = useState<ActiveReview>();
  const [panelError, setPanelError] = useState<
    "load_failed" | "count_unavailable" | "change_failed"
  >();
  const [announcement, setAnnouncement] = useState<{
    key: "enabled" | "disabled" | "period_saved";
    days?: number;
    sequence: number;
  }>();
  const [validationAttempted, setValidationAttempted] = useState(false);
  const [suggestionChanged, setSuggestionChanged] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const mounted = useRef(true);
  const nextDialogGeneration = useRef(0);
  const cancelledReviews = useRef(new Set<string>());
  const resolvedReviews = useRef(new Set<string>());

  useEffect(() => {
    mounted.current = true;
    let current = true;
    void client
      .call("history.retention_get", {})
      .then((value) => {
        if (!current) return;
        setPolicy(value);
        setDraftDays(String(value.specifiedDays ?? value.suggestedDays));
      })
      .catch(() => {
        if (current) setPanelError("load_failed");
      })
      .finally(() => {
        if (current) setLoading(false);
      });
    return () => {
      current = false;
      mounted.current = false;
    };
  }, [client]);

  useLayoutEffect(() => {
    if (isOpen) titleRef.current?.focus({ preventScroll: true });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen || activeReview) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || isImeProcessingKey(event) || busy) return;
      event.preventDefault();
      event.stopPropagation();
      setIsOpen(false);
      triggerRef.current?.focus();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [isOpen, activeReview, busy]);

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

  function closePanel() {
    if (activeReview) cancelReview(activeReview.review.reviewRef);
    setActiveReview(undefined);
    setIsOpen(false);
    triggerRef.current?.focus();
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
    const days = enabled
      ? parseDays(String(policy.specifiedDays ?? policy.suggestedDays))
      : undefined;
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

  return {
    policy,
    draftDays,
    loading,
    busy,
    isOpen,
    activeReview,
    panelError,
    announcement,
    validationAttempted,
    suggestionChanged,
    triggerRef,
    titleRef,
    inputRef,
    openPanel: () => setIsOpen(true),
    closePanel,
    setDraftDays: (value: string) => {
      setDraftDays(value);
      if (
        policy?.specifiedDays === undefined &&
        value !== String(policy?.suggestedDays)
      )
        setSuggestionChanged(true);
    },
    changeEnabled,
    applyPeriod,
    confirmReview,
    cancelActiveReview,
  };
}

type RetentionController = ReturnType<typeof useHistoryRetention>;

function reviewCount(value: string, text: ReturnType<typeof useLocaleText>) {
  return text("ui.history.retention.count", "{count} records", {
    count: BigInt(value),
  });
}

function RetentionReviewContent({
  activeReview,
  targetRange,
  targetMessage,
}: {
  activeReview: ActiveReview;
  targetRange: string;
  targetMessage: string;
}) {
  const text = useLocaleText();
  const review = activeReview.review;
  const rows = [
    {
      label: text("ui.history.retention.to_remove", "History to remove"),
      value: reviewCount(review.targetCount, text),
    },
    {
      label: text("ui.history.retention.target_period", "Target period"),
      value: targetRange,
    },
    ...(review.heldCount !== "0"
      ? [
          {
            label: text("ui.history.retention.held", "History kept this time"),
            value: reviewCount(review.heldCount, text),
            notes: [
              text(
                "ui.history.retention.held_note",
                "These records have unfinished restoration or follow-up work.",
              ),
            ],
          },
        ]
      : []),
    ...(review.undoInfoRemovalCount !== "0"
      ? [
          {
            label: text(
              "ui.history.retention.undo_info",
              "Undo information also removed",
            ),
            value: reviewCount(review.undoInfoRemovalCount, text),
            notes: [
              text(
                "ui.history.retention.undo_info_note",
                "Information used to undo actions in this session.",
              ),
            ],
          },
        ]
      : []),
  ];
  const hasTargets = review.targetCount !== "0";

  return (
    <section className="du-history-retention__dialog">
      {activeReview.stale && (
        <Notice>
          {text(
            "ui.history.retention.review_stale",
            "History changed in the meantime, so the counts were updated.",
          )}
        </Notice>
      )}
      {activeReview.dialogError && (
        <Notice tone="error">
          {activeReview.dialogError === "save_failed"
            ? text(
                "ui.history.retention.save_failed",
                "Settings couldn't be saved. Try again.",
              )
            : text(
                "ui.history.retention.change_failed",
                "The retention setting couldn't be changed. Try again.",
              )}
        </Notice>
      )}
      {hasTargets ? (
        <Readout
          label={text(
            "ui.history.retention.review_summary",
            "History removal summary",
          )}
          rows={rows}
        />
      ) : (
        <Text>{targetMessage}</Text>
      )}
      <Text kind="muted">
        {text(
          "ui.history.retention.disk_unchanged",
          "Files on disk and the Recycle Bin don't change.",
        )}
      </Text>
    </section>
  );
}

function RetentionReviewDialog({
  activeReview,
  busy,
  onConfirm,
  onCancel,
}: {
  activeReview: ActiveReview;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampText();
  const { review, intent } = activeReview;
  const days = intent.request.days ?? 1;
  const confirmIsEnable = review.changeKind === "enable";
  const confirmLabel = confirmIsEnable
    ? text("ui.history.retention.enable_action", "Turn on")
    : text("ui.history.retention.shorten_action", "Shorten");
  const confirmTitle = confirmIsEnable
    ? text("ui.history.retention.enable_title", "Turn on automatic cleanup?")
    : text(
        "ui.history.retention.shorten_title",
        "Shorten retention to {days} days?",
        { days: BigInt(days) },
      );
  const start = review.targetEarliestAtMs;
  const end = review.targetLatestAtMs;
  const targetRange =
    start != null && end != null
      ? `${timestamp(start, () => text("ui.history.retention.time_unknown", "Time unavailable"))} – ${timestamp(end, () => text("ui.history.retention.time_unknown", "Time unavailable"))}`
      : text("ui.history.retention.time_unknown", "Time unavailable");
  const targetMessage = text(
    "ui.history.retention.zero_targets",
    "Nothing will be removed now. History older than {days} days will be cleaned up in the future.",
    { days: BigInt(days) },
  );
  const hasTargets = review.targetCount !== "0";

  return (
    <ConfirmDialog
      key={activeReview.generation}
      title={confirmTitle}
      confirmLabel={confirmLabel}
      cancelLabel={text("ui.history.retention.cancel", "Cancel")}
      confirmVariant={hasTargets ? "danger" : "secondary"}
      cancelVariant="primary"
      isPending={busy}
      onConfirm={onConfirm}
      onCancel={onCancel}
    >
      <RetentionReviewContent
        activeReview={activeReview}
        targetRange={targetRange}
        targetMessage={targetMessage}
      />
    </ConfirmDialog>
  );
}

/** Renders the non-modal history retention side panel and its confirmation. */
function RetentionPolicyNotices({
  controller,
}: {
  controller: RetentionController;
}) {
  const text = useLocaleText();
  const policyWarning =
    controller.policy?.unavailableReason === "history.policy_unreadable"
      ? text(
          "ui.history.retention.policy_unreadable",
          "Saved retention settings couldn't be read, so automatic cleanup is off.",
        )
      : controller.policy?.unavailableReason === "history.policy_invalid"
        ? text(
            "ui.history.retention.policy_invalid",
            "Saved retention settings aren't valid, so automatic cleanup is off.",
          )
        : undefined;
  const panelError =
    controller.panelError === "load_failed"
      ? text(
          "ui.history.retention.load_failed",
          "Retention settings couldn't be loaded. Try again.",
        )
      : controller.panelError
        ? localizedError(controller.panelError, text)
        : undefined;

  return (
    <>
      {policyWarning && <Notice tone="warning">{policyWarning}</Notice>}
      {panelError && <Notice tone="error">{panelError}</Notice>}
    </>
  );
}

function RetentionPeriodControl({
  controller,
}: {
  controller: RetentionController;
}) {
  const text = useLocaleText();
  const { policy } = controller;
  const days = parseDays(controller.draftDays);
  const unchanged =
    days !== undefined &&
    days === (policy?.specifiedDays ?? policy?.suggestedDays);
  const fieldError =
    controller.validationAttempted && days === undefined
      ? text(
          "ui.history.retention.invalid_days",
          "Enter a whole number of 1 or more.",
        )
      : undefined;
  const showSuggested =
    policy?.specifiedDays === undefined &&
    !controller.suggestionChanged &&
    controller.draftDays === String(policy?.suggestedDays);

  return (
    <div className="du-history-retention__period">
      <IntegerField
        label={text("ui.history.retention.keep_for", "Keep history for")}
        value={controller.draftDays}
        unit={text("ui.history.retention.days", "days")}
        after={
          showSuggested ? (
            <StatusBadge>
              {text("ui.history.retention.suggested", "Suggested")}
            </StatusBadge>
          ) : undefined
        }
        error={fieldError}
        inputRef={controller.inputRef}
        isDisabled={controller.loading || controller.busy || !policy}
        onChange={controller.setDraftDays}
      />
      <Action
        isDisabled={
          controller.loading || controller.busy || !policy || unchanged
        }
        onPress={controller.applyPeriod}
      >
        {text("ui.history.retention.apply", "Apply")}
      </Action>
    </div>
  );
}

function RetentionAnnouncement({
  announcement,
}: {
  announcement:
    | {
        key: "enabled" | "disabled" | "period_saved";
        days?: number;
        sequence: number;
      }
    | undefined;
}) {
  const text = useLocaleText();
  if (!announcement) return null;
  let message: string;
  if (announcement.key === "period_saved")
    message = text(
      "ui.history.retention.period_saved",
      "Retention was changed to {days} days.",
      { days: BigInt(announcement.days ?? 1) },
    );
  else if (announcement.key === "enabled")
    message = text(
      "ui.history.retention.enabled",
      "Automatic cleanup was turned on.",
    );
  else
    message = text(
      "ui.history.retention.disabled",
      "Automatic cleanup was turned off.",
    );
  return <LiveStatus key={announcement.sequence}>{message}</LiveStatus>;
}

/** Renders the non-modal history retention side panel and its confirmation. */
export function HistoryRetentionPanel({
  controller,
}: {
  controller: RetentionController;
}) {
  const text = useLocaleText();
  const { policy, activeReview } = controller;

  return (
    <aside
      id="du-history-retention-panel"
      className="du-history-retention"
      aria-labelledby="du-history-retention-title"
      aria-hidden={!controller.isOpen || undefined}
      inert={!controller.isOpen}
      data-open={controller.isOpen || undefined}
    >
      <header className="du-history-retention__header">
        <Heading
          level={2}
          id="du-history-retention-title"
          ref={controller.titleRef}
          programmaticFocus
        >
          {text("ui.history.retention.panel_title", "History retention")}
        </Heading>
        <Action
          variant="quiet"
          icon="close"
          isDisabled={controller.busy}
          aria-label={text(
            "ui.history.retention.close",
            "Close retention settings",
          )}
          onPress={controller.closePanel}
        />
      </header>
      <div className="du-history-retention__body">
        <RetentionPolicyNotices controller={controller} />
        <Text>
          {text(
            "ui.history.retention.description",
            "Only task history is cleaned up. Files on disk and the Recycle Bin don't change.",
          )}
        </Text>
        {controller.loading && (
          <Progress
            label={text(
              "ui.history.retention.loading",
              "Loading retention settings",
            )}
          />
        )}
        <SwitchField
          label={text(
            "ui.history.retention.auto_cleanup",
            "Clean up automatically",
          )}
          isSelected={policy?.enabled ?? false}
          isDisabled={controller.loading || controller.busy || !policy}
          onChange={controller.changeEnabled}
        />
        <RetentionPeriodControl controller={controller} />
        <RetentionAnnouncement announcement={controller.announcement} />
      </div>
      {activeReview && (
        <RetentionReviewDialog
          activeReview={activeReview}
          busy={controller.busy}
          onConfirm={controller.confirmReview}
          onCancel={controller.cancelActiveReview}
        />
      )}
    </aside>
  );
}

export function retentionHeaderText(
  policy: HistoryRetentionPolicy | undefined,
  text: ReturnType<typeof useLocaleText>,
) {
  if (!policy?.enabled) return undefined;
  return text(
    "ui.history.retention.header_line",
    "History older than {days} days is cleaned up automatically.",
    { days: BigInt(policy.specifiedDays ?? policy.suggestedDays) },
  );
}
