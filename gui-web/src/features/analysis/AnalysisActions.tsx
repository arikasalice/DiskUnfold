import { useLayoutEffect, useRef, useState } from "react";
import type { Ref, RefObject } from "react";
import {
  Action,
  LiveStatus,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
import type { JobState, ScopeReview } from "../../ipc";
import type { ScanControl } from "../../state/analysis-scan";

export function scopeNoticeText(key: string): string {
  switch (key) {
    case "scope.overlap_merged":
      return "Duplicate paths were combined.";
    case "scope.alias_merged":
      return "Paths to the same location were combined.";
    case "scope.descendant_absorbed":
      return "A selected folder is already inside another selected folder.";
    case "scope.correspondence_unknown":
      return "Some path relationships could not be confirmed.";
    case "scope.excluded":
      return "An exclusion removed a selected folder from the scan.";
    case "scope.exclusion_ineffective":
      return "An exclusion falls outside the selected folders.";
    case "scan.path_vanished":
      return "A selected folder no longer exists. Check the path and review the scope again.";
    case "scan.path_denied":
      return "Access to a selected folder was denied. Check its permissions and review the scope again.";
    case "scan.disconnected":
      return "A selected drive was disconnected. Reconnect it and review the scope again.";
    case "path.unresolvable":
      return "A selected folder path could not be resolved. Check the path and review the scope again.";
    default:
      return "A selected path needs attention. Check the folder paths and review the scope again.";
  }
}

function ReviewNotices({
  review,
  ready,
  current,
  problem,
  problemId,
}: {
  review: ScopeReview | undefined;
  ready: boolean;
  current: boolean;
  problem: string | undefined;
  problemId: string;
}) {
  if (problem)
    return (
      <Notice tone="error" id={problemId}>
        {problem}
      </Notice>
    );
  if (!review) return null;
  if (!current)
    return (
      <Notice tone="info" id={problemId}>
        The current paths changed.
      </Notice>
    );
  if (ready)
    return review.notices.map((notice) => (
      <Notice key={notice}>{scopeNoticeText(notice)}</Notice>
    ));

  const messages = new Set(review.notices.map(scopeNoticeText));
  for (const target of review.targets) {
    if (target.access !== "accessible" && target.reasonKey)
      messages.add(scopeNoticeText(target.reasonKey));
  }
  if (!messages.size)
    messages.add(
      "One or more selected paths could not be verified. Review all paths before starting.",
    );
  return (
    <Notice tone="error" id={problemId}>
      {[...messages].map((message) => (
        <Text key={message}>{message}</Text>
      ))}
    </Notice>
  );
}

export function ScopeReviewFeedback({
  review,
  ready,
  current,
  latestAccepted,
  problem,
  problemId,
}: {
  review: ScopeReview | undefined;
  ready: boolean;
  current: boolean;
  latestAccepted: boolean;
  problem: string | undefined;
  problemId: string;
}) {
  if (!review && !problem) return null;
  return (
    <section
      className="du-review-feedback"
      aria-label={ready ? "Reviewed scope" : "Scope review problem"}
    >
      <ReviewNotices
        review={review}
        ready={ready}
        current={current}
        problem={problem}
        problemId={problemId}
      />
      {review && !latestAccepted && (
        <Text kind="muted">
          Previously reviewed scope. Review the current paths before starting.
        </Text>
      )}
      {review && (
        <Readout
          label={
            latestAccepted
              ? "Scope paths and boundary"
              : "Previously reviewed scope"
          }
          rows={[
            {
              label: "Included paths",
              value: review.scope.includes
                .map((path) => path.display)
                .join(", "),
            },
            { label: "Boundary", value: "Current volume" },
          ]}
        />
      )}
    </section>
  );
}

function scopeActionState({
  checkingDrop,
  composing,
  reviewing,
  working,
  running,
  reviewEnabled,
  startEnabled,
}: {
  checkingDrop: boolean;
  composing: boolean;
  reviewing: boolean;
  working: boolean;
  running: boolean;
  reviewEnabled: boolean;
  startEnabled: boolean;
}) {
  const blocked = reviewing || working || running;
  const starting = working && !running;
  return {
    starting,
    reviewDisabled:
      composing || reviewing || working || running || !reviewEnabled,
    reviewPending: reviewing || (checkingDrop && !blocked && reviewEnabled),
    startDisabled:
      composing || reviewing || (!startEnabled && !starting && !running),
    startPending:
      starting || running || (checkingDrop && !blocked && startEnabled),
  };
}

export function ScopeActions({
  reviewButton,
  checkingDrop,
  composing,
  reviewing,
  working,
  running,
  reviewEnabled,
  startEnabled,
  startLabel = "Start scan",
  onReview,
  onStart,
  onCancel,
}: {
  reviewButton: Ref<HTMLButtonElement>;
  checkingDrop: boolean;
  composing: boolean;
  reviewing: boolean;
  working: boolean;
  running: boolean;
  reviewEnabled: boolean;
  startEnabled: boolean;
  startLabel?: string;
  onReview: () => void;
  onStart: () => void;
  onCancel: () => void;
}) {
  const {
    starting,
    reviewDisabled,
    reviewPending,
    startDisabled,
    startPending,
  } = scopeActionState({
    checkingDrop,
    composing,
    reviewing,
    working,
    running,
    reviewEnabled,
    startEnabled,
  });
  return (
    <footer className="du-target-editor__footer">
      <LiveStatus>
        {checkingDrop && <Text>Checking dropped folder…</Text>}
      </LiveStatus>
      {reviewing && <Progress label="Reviewing scope" />}
      {starting && <Progress label="Starting scan" />}
      <div className="du-target-editor__actions">
        <Action isDisabled={working || running} onPress={onCancel}>
          Cancel selection
        </Action>
        <Action
          ref={reviewButton}
          icon="folder"
          variant={startEnabled ? "secondary" : "primary"}
          isDisabled={reviewDisabled}
          isPending={reviewPending}
          onPress={onReview}
        >
          {reviewing ? "Reviewing scope…" : "Review scope"}
        </Action>
        <Action
          icon="scan"
          variant={startEnabled ? "primary" : "secondary"}
          isDisabled={startDisabled}
          isPending={startPending}
          onPress={onStart}
        >
          {starting ? "Starting scan…" : startLabel}
        </Action>
      </div>
    </footer>
  );
}

function useResultControlFocus(
  running: boolean,
  command: "pause" | "resume" | undefined,
  statusFocus: RefObject<HTMLDivElement | null>,
) {
  const suspend = useRef<HTMLButtonElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const [previous, setPrevious] = useState({ running, command });

  useLayoutEffect(() => {
    if (previous.running === running && previous.command === command) return;
    const active = document.activeElement;
    if (
      active &&
      ((!running && active === cancel.current) ||
        (!command && active === suspend.current))
    )
      statusFocus.current?.focus({ preventScroll: true });
    setPrevious({ running, command });
  }, [command, previous, running, statusFocus]);

  // Keep outgoing controls mounted until their focus can be checked.
  return {
    suspend,
    cancel,
    showCancel: running || previous.running,
    shownCommand: command ?? previous.command,
  };
}

export function ResultActions({
  version,
  onBrowse,
  trigger,
  running,
  state,
  pending,
  onControl,
  statusFocus,
}: {
  version: string | undefined;
  onBrowse: ((version: string) => void) | undefined;
  trigger: Ref<HTMLButtonElement> | undefined;
  running: boolean;
  state: JobState | undefined;
  pending: ScanControl | undefined;
  onControl: (command: ScanControl) => void;
  statusFocus: RefObject<HTMLDivElement | null>;
}) {
  const browse = version && onBrowse && (
    <Action
      ref={trigger ?? null}
      icon="folder"
      variant="primary"
      onPress={() => onBrowse(version)}
    >
      Browse folders
    </Action>
  );
  const command = !running
    ? undefined
    : state === "running"
      ? "pause"
      : state === "pause_requested" || state === "paused"
        ? "resume"
        : undefined;
  const cancelling = pending === "cancel" || state === "cancel_requested";
  const { suspend, cancel, showCancel, shownCommand } = useResultControlFocus(
    running,
    command,
    statusFocus,
  );
  if (!browse && !showCancel) return null;
  return (
    <div className="du-result__actions">
      {browse}
      {shownCommand && (
        <Action
          ref={suspend}
          isPending={!command || cancelling || pending === command}
          onPress={() => onControl(shownCommand)}
        >
          {shownCommand === "pause" ? "Pause scan" : "Resume scan"}
        </Action>
      )}
      {showCancel && (
        <Action
          ref={cancel}
          isPending={!running || cancelling}
          onPress={() => onControl("cancel")}
        >
          Cancel scan
        </Action>
      )}
    </div>
  );
}
