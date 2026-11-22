import { ConfirmDialog, Notice, Readout, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import type { ActiveReview } from "./history-retention-model";

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
      label: text("ui.history.retention.confirm_period", "Retention period"),
      value: text("ui.history.retention.confirm_days", "{days} days", {
        days: BigInt(activeReview.intent.request.days ?? 1),
      }),
    },
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
      <Readout
        label={text(
          "ui.history.retention.review_summary",
          "History removal summary",
        )}
        rows={rows.slice(0, hasTargets ? rows.length : 1)}
      />
      {!hasTargets && <Text>{targetMessage}</Text>}
      <Text kind="muted">
        {text(
          "ui.history.retention.disk_unchanged",
          "Files on disk and the Recycle Bin don't change.",
        )}
      </Text>
    </section>
  );
}

/** Renders the confirmation for a reviewed history retention change. */
export function HistoryRetentionDialog({
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
