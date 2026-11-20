import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  Action,
  ConfirmDialog,
  Heading,
  LiveStatus,
  MiddleEllipsisText,
  Notice,
  Progress,
  StatusBadge,
  Text,
  isImeProcessingKey,
} from "../../components";
import { scanJobReasonText } from "../../ipc";
import type { AnalysisJobListRow } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import { displayInteger, useRegionalFormat } from "../../i18n/regional-format";
import type {
  JobCenterCard,
  JobCenterCommand,
  JobCenterFeedback,
  JobCenterSnapshot,
} from "../../state/job-center";
import "../../theme/job-center.css";

/** The task panel view shared by the app and its fixed gallery examples. */
export function JobCenterPresenter({
  isOpen,
  trigger,
  snapshot,
  onClose,
  onOpenHistory,
  onOpenJob,
  onRetry,
  onControl,
  announcement,
  initialCancelJobRef,
}: {
  isOpen: boolean;
  trigger: RefObject<HTMLButtonElement | null>;
  snapshot: JobCenterSnapshot;
  onClose: () => void;
  onOpenHistory: () => void;
  onOpenJob: (card: JobCenterCard) => void;
  onRetry: () => void;
  onControl: (jobRef: string, command: JobCenterCommand) => void;
  announcement?: { id: number; message: string };
  initialCancelJobRef?: string;
}) {
  const text = useLocaleText();
  const title = useRef<HTMLHeadingElement>(null);
  const [cancelJob, setCancelJob] = useState(initialCancelJobRef);
  const announcedFeedback = [...snapshot.feedback.entries()];

  useLayoutEffect(() => {
    if (isOpen) title.current?.focus({ preventScroll: true });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (
        event.key !== "Escape" ||
        cancelJob !== undefined ||
        isImeProcessingKey(event)
      )
        return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
      trigger.current?.focus();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [cancelJob, isOpen, onClose, trigger]);

  const close = () => {
    setCancelJob(undefined);
    onClose();
    trigger.current?.focus();
  };
  const selected = cancelJob
    ? snapshot.cards.find((card) => card.row.jobRef === cancelJob)
    : undefined;

  return (
    <aside
      id="du-job-center-panel"
      className="du-job-center"
      aria-labelledby="du-job-center-title"
      aria-hidden={!isOpen || undefined}
      inert={!isOpen}
      data-open={isOpen || undefined}
    >
      <header className="du-job-center__header">
        <Heading
          level={2}
          id="du-job-center-title"
          ref={title}
          programmaticFocus
        >
          {text("ui.job_center.title", "Tasks")}
        </Heading>
        <Action
          variant="quiet"
          icon="close"
          aria-label={text("ui.job_center.close", "Close tasks")}
          onPress={close}
        />
      </header>
      {snapshot.resyncing && (
        <Text kind="muted">
          {text("ui.job_center.resyncing", "Updating the task list…")}
        </Text>
      )}
      <div className="du-job-center__body">
        {snapshot.error && (
          <Notice tone="error">
            {text(
              "ui.job_center.load_failed",
              "The task list couldn't be loaded.",
            )}{" "}
            <Action onPress={onRetry} isPending={snapshot.loading}>
              {text("ui.job_center.retry", "Retry")}
            </Action>
          </Notice>
        )}
        {snapshot.loading && snapshot.cards.length === 0 && (
          <Progress label={text("ui.job_center.loading", "Loading tasks")} />
        )}
        <section
          className="du-job-center__section"
          aria-labelledby="du-job-center-in-progress"
        >
          <Heading level={3} id="du-job-center-in-progress">
            {text("ui.job_center.in_progress", "In progress")}
          </Heading>
          {snapshot.inProgress.length === 0 &&
          !snapshot.loading &&
          !snapshot.error ? (
            <Text kind="muted">
              {text("ui.job_center.empty_in_progress", "No tasks are running.")}
            </Text>
          ) : (
            <div className="du-job-center__cards">
              {snapshot.inProgress.map((card) => (
                <ActiveCard
                  key={card.row.jobRef}
                  card={card}
                  pending={snapshot.pending.get(card.row.jobRef)}
                  feedback={snapshot.feedback.get(card.row.jobRef)}
                  onOpen={() => onOpenJob(card)}
                  onCancel={() => setCancelJob(card.row.jobRef)}
                  onControl={(command) => onControl(card.row.jobRef, command)}
                />
              ))}
            </div>
          )}
        </section>
        {snapshot.recentlyFinished.length > 0 && (
          <section
            className="du-job-center__section"
            aria-labelledby="du-job-center-recently-finished"
          >
            <Heading level={3} id="du-job-center-recently-finished">
              {text("ui.job_center.recently_finished", "Recently finished")}
            </Heading>
            <div className="du-job-center__cards">
              {snapshot.recentlyFinished.map((card) => (
                <FinishedCard
                  key={card.row.jobRef}
                  card={card}
                  feedback={snapshot.feedback.get(card.row.jobRef)}
                  onOpen={() => onOpenJob(card)}
                />
              ))}
            </div>
          </section>
        )}
      </div>
      <footer className="du-job-center__footer">
        <Action variant="quiet" onPress={onOpenHistory}>
          {text("ui.job_center.view_all_history", "View all task history")}
        </Action>
      </footer>
      <div className="du-announcer">
        {announcedFeedback.map(([jobRef, result]) => (
          <LiveStatus key={`${jobRef}:${result.id}`}>
            {feedbackText(result, text)}
          </LiveStatus>
        ))}
        {isOpen && announcement && (
          <LiveStatus key={announcement.id}>{announcement.message}</LiveStatus>
        )}
      </div>
      {selected && (
        <ConfirmDialog
          title={text("ui.job_center.cancel_title", "Cancel {kind}?", {
            kind: kindLabel(selected.row.kind, text),
          })}
          confirmLabel={text("ui.job_center.cancel_confirm", "Cancel task")}
          cancelLabel={text("ui.job_center.continue", "Continue")}
          onConfirm={() => {
            onControl(selected.row.jobRef, "cancel");
            setCancelJob(undefined);
          }}
          onCancel={() => setCancelJob(undefined)}
        >
          {selected.terminal?.unprocessedSummary ? (
            <Text>{selected.terminal.unprocessedSummary}</Text>
          ) : null}
        </ConfirmDialog>
      )}
    </aside>
  );
}

function ActiveCard({
  card,
  pending,
  feedback,
  onOpen,
  onCancel,
  onControl,
}: {
  card: JobCenterCard;
  pending: JobCenterCommand | undefined;
  feedback: JobCenterFeedback | undefined;
  onOpen: () => void;
  onCancel: () => void;
  onControl: (command: JobCenterCommand) => void;
}) {
  const text = useLocaleText();
  const waiting = card.row.state === "queued";
  const supported = card.row.supportedControls;
  const showPause =
    supported.includes("pause") &&
    (card.row.state === "running" || pending === "pause");
  const showResume =
    supported.includes("resume") &&
    (card.row.state === "paused" || pending === "resume");
  const showCancel =
    supported.includes("cancel") &&
    ((!isTerminalState(card.row.state) &&
      card.row.state !== "cancel_requested") ||
      pending === "cancel");

  return (
    <article className="du-job-center__card">
      <div className="du-job-center__card-head">
        <div className="du-job-center__kind-status">
          <Text inline className="du-job-center__kind">
            {kindLabel(card.row.kind, text)}
          </Text>
          <StatusBadge tone={activeTone(card.row.state)}>
            {statusLabel(card.row.state, text)}
          </StatusBadge>
        </div>
        <div className="du-job-center__controls">
          {showPause && (
            <Action
              icon="pause"
              isPending={pending === "pause"}
              isDisabled={pending !== undefined}
              onPress={() => onControl("pause")}
            >
              {text("ui.job_center.pause", "Pause")}
            </Action>
          )}
          {showResume && (
            <Action
              icon="resume"
              isPending={pending === "resume"}
              isDisabled={pending !== undefined}
              onPress={() => onControl("resume")}
            >
              {text("ui.job_center.resume", "Resume")}
            </Action>
          )}
          {showCancel && (
            <Action
              icon="cancel"
              isPending={pending === "cancel"}
              isDisabled={pending !== undefined}
              onPress={onCancel}
            >
              {text("ui.job_center.cancel", "Cancel")}
            </Action>
          )}
        </div>
      </div>
      <Target row={card.row} />
      <div className="du-job-center__progress">
        {waiting ? (
          <Waiting reason={card.row.waitReason} />
        ) : (
          <JobProgress card={card} />
        )}
        <div className="du-job-center__state-line">
          {pending && <Text kind="muted">{pendingText(pending, text)}</Text>}
          <Action variant="quiet" onPress={onOpen}>
            {text("ui.job_center.view", "View")}
          </Action>
        </div>
      </div>
      {feedback && <FeedbackLine feedback={feedback} />}
    </article>
  );
}

function FinishedCard({
  card,
  feedback,
  onOpen,
}: {
  card: JobCenterCard;
  feedback: JobCenterFeedback | undefined;
  onOpen: () => void;
}) {
  const text = useLocaleText();
  const state = card.terminal?.state ?? card.row.state;
  return (
    <article className="du-job-center__card du-job-center__card--finished">
      <div className="du-job-center__card-head">
        <div className="du-job-center__kind-status">
          <Text inline className="du-job-center__kind">
            {kindLabel(card.row.kind, text)}
          </Text>
          <StatusBadge tone={finishedTone(state)}>
            {finishedLabel(state, text)}
          </StatusBadge>
        </div>
        <Action variant="quiet" onPress={onOpen}>
          {text("ui.job_center.view_result", "View result")}
        </Action>
      </div>
      <Target row={card.row} />
      {feedback && <FeedbackLine feedback={feedback} />}
    </article>
  );
}

function Target({ row }: { row: AnalysisJobListRow }) {
  const text = useLocaleText();
  const root = row.scopeFirstRoot;
  const more = Math.max(0, (row.scopeRootCount ?? 1) - 1);
  return (
    <div className="du-job-center__target">
      <MiddleEllipsisText text={root || row.scopeSummary} />
      {root && more > 0 && (
        <Text kind="muted">
          {text("ui.history.roots_more", "and {count} more", {
            count: BigInt(more),
          })}
        </Text>
      )}
    </div>
  );
}

function JobProgress({ card }: { card: JobCenterCard }) {
  const text = useLocaleText();
  const regional = useRegionalFormat();
  const progress = card.progress;
  const unit = progress?.unit ?? "items";
  const formattedCount = displayInteger(
    BigInt(progress?.completed ?? 0),
    regional,
  );
  const determinate =
    progress?.totalKnown === true && progress.total !== undefined
      ? progress
      : undefined;
  const label = determinate
    ? text(
        unit === "bytes"
          ? "ui.job_center.progress_bytes"
          : "ui.job_center.progress_items",
        unit === "bytes"
          ? "{count} / {total} bytes"
          : "{count} / {total} items",
        {
          count: formattedCount,
          total: displayInteger(BigInt(determinate.total ?? 0), regional),
        },
      )
    : text(
        unit === "bytes"
          ? "ui.job_center.checked_bytes"
          : "ui.job_center.checked_items",
        unit === "bytes" ? "Checked {count} bytes" : "Checked {count} items",
        { count: formattedCount },
      );
  return (
    <div className="du-job-center__progress-value">
      <Text kind="muted">{label}</Text>
      <Progress
        label={label}
        {...(determinate
          ? { value: determinate.completed, maxValue: determinate.total }
          : {})}
        paused={card.row.state === "paused"}
        showLabel={false}
      />
    </div>
  );
}

function Waiting({ reason }: { reason: AnalysisJobListRow["waitReason"] }) {
  const text = useLocaleText();
  const message =
    reason === "queue_full"
      ? text(
          "ui.job_center.wait_other_task",
          "Waiting for another task to finish…",
        )
      : reason === "io_wait"
        ? text("ui.job_center.wait_io", "Waiting for file access…")
        : reason === "device_cap"
          ? text("ui.job_center.wait_device", "Waiting for device capacity…")
          : reason === "backpressure"
            ? text("ui.job_center.wait_backpressure", "Waiting for capacity…")
            : text("ui.job_center.waiting", "Waiting…");
  return <Text kind="muted">{message}</Text>;
}

function FeedbackLine({ feedback }: { feedback: JobCenterFeedback }) {
  const text = useLocaleText();
  return (
    <div
      className={`du-job-center__feedback du-job-center__feedback--${feedback.kind}`}
    >
      <Text>{feedbackText(feedback, text)}</Text>
    </div>
  );
}

function feedbackText(
  feedback: JobCenterFeedback,
  text: ReturnType<typeof useLocaleText>,
) {
  if (feedback.kind === "error")
    return text(
      "ui.job_center.control_send_failed",
      "The request couldn't be sent. Try again.",
    );
  const key = `ui.job_center.control_unavailable_${feedback.command}`;
  const fallback =
    feedback.command === "pause"
      ? "This task can't be paused right now."
      : feedback.command === "resume"
        ? "This task can't be resumed right now."
        : "This task can't be canceled right now.";
  return (
    <>
      {text(key, fallback)}
      {feedback.reasonKey && (
        <> {scanJobReasonText(feedback.reasonKey, text)}</>
      )}
    </>
  );
}

function kindLabel(kind: string, text: ReturnType<typeof useLocaleText>) {
  if (kind === "scan.metadata")
    return text("ui.history.kind_disk_analysis", "Disk analysis");
  if (kind === "duplicate.check")
    return text("ui.history.kind_duplicate_check", "Duplicate check");
  return text("ui.history.kind_unavailable", "Operation type unavailable");
}

function statusLabel(
  state: AnalysisJobListRow["state"],
  text: ReturnType<typeof useLocaleText>,
) {
  if (state === "queued") return text("ui.job_center.state_queued", "Queued");
  if (state === "paused") return text("ui.job_center.state_paused", "Paused");
  return text("ui.job_center.state_running", "In progress");
}

function pendingText(
  command: JobCenterCommand,
  text: ReturnType<typeof useLocaleText>,
) {
  if (command === "pause") return text("ui.job_center.pausing", "Pausing…");
  if (command === "resume") return text("ui.job_center.resuming", "Resuming…");
  return text("ui.job_center.canceling", "Canceling…");
}

function activeTone(state: AnalysisJobListRow["state"]): "neutral" | "info" {
  return state === "paused" ? "neutral" : "info";
}

function finishedLabel(
  state: AnalysisJobListRow["state"],
  text: ReturnType<typeof useLocaleText>,
) {
  if (state === "done") return text("ui.history.state_completed", "Completed");
  if (state === "partial")
    return text("ui.history.state_partial", "Partially completed");
  if (state === "failed") return text("ui.history.state_failed", "Failed");
  return text("ui.history.state_cancelled", "Cancelled");
}

function finishedTone(
  state: AnalysisJobListRow["state"],
): "good" | "warning" | "neutral" {
  if (state === "done") return "good";
  if (state === "partial" || state === "failed") return "warning";
  return "neutral";
}

function isTerminalState(state: string) {
  return ["done", "partial", "failed", "cancelled"].includes(state);
}
