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
import type { JobState, ScopeDraft, ScopeReview } from "../../ipc";
import type { ScanControl } from "../../state/analysis-scan";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";

export function scopeNoticeText(
  key: string,
  text: LocaleText = (messageKey, fallback) => fallback || messageKey,
): string {
  switch (key) {
    case "scope.overlap_merged":
      return text(
        "ui.analysis.scope_overlap",
        "Duplicate paths were combined.",
      );
    case "scope.alias_merged":
      return text(
        "ui.analysis.scope_alias",
        "Paths to the same location were combined.",
      );
    case "scope.descendant_absorbed":
      return text(
        "ui.analysis.scope_descendant",
        "A selected folder is already inside another selected folder.",
      );
    case "scope.correspondence_unknown":
      return text(
        "ui.analysis.scope_uncertain",
        "Some path relationships could not be confirmed.",
      );
    case "scope.excluded":
      return text(
        "ui.analysis.scope_excluded",
        "An exclusion removed a selected folder from the scan.",
      );
    case "scope.exclusion_ineffective":
      return text(
        "ui.analysis.scope_exclusion_outside",
        "An exclusion falls outside the selected folders.",
      );
    case "scan.path_vanished":
      return text(
        "ui.analysis.scope_missing",
        "A selected folder no longer exists. Check the path and review the scope again.",
      );
    case "scan.path_denied":
      return text(
        "ui.analysis.scope_denied",
        "Access to a selected folder was denied. Check its permissions and review the scope again.",
      );
    case "scan.disconnected":
      return text(
        "ui.analysis.scope_disconnected",
        "A selected drive was disconnected. Reconnect it and review the scope again.",
      );
    case "path.unresolvable":
      return text(
        "ui.analysis.scope_unresolvable",
        "A selected folder path could not be resolved. Check the path and review the scope again.",
      );
    default:
      return text(
        "ui.analysis.scope_needs_review",
        "A selected path needs attention. Check the folder paths and review the scope again.",
      );
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
  const text = useLocaleText();
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
        {text("ui.analysis.scope_changed", "The current paths changed.")}
      </Notice>
    );
  if (ready)
    return review.notices.map((notice) => (
      <Notice key={notice}>{scopeNoticeText(notice, text)}</Notice>
    ));

  const messages = new Set(
    review.notices.map((notice) => scopeNoticeText(notice, text)),
  );
  for (const target of review.targets) {
    if (target.access !== "accessible" && target.reasonKey)
      messages.add(scopeNoticeText(target.reasonKey, text));
  }
  if (!messages.size)
    messages.add(
      text(
        "ui.analysis.scope_unverified",
        "One or more selected paths could not be verified. Review all paths before starting.",
      ),
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
  const text = useLocaleText();
  if (!review && !problem) return null;
  return (
    <section
      className="du-review-feedback"
      aria-label={
        ready
          ? text("ui.analysis.reviewed_scope", "Reviewed scope")
          : text("ui.analysis.scope_problem", "Scope review problem")
      }
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
          {text(
            "ui.analysis.review_current_paths",
            "Previously reviewed scope. Review the current paths before starting.",
          )}
        </Text>
      )}
      {review && (
        <Readout
          label={
            latestAccepted
              ? text("ui.analysis.scope_boundary", "Scope paths and boundary")
              : text("ui.analysis.previous_scope", "Previously reviewed scope")
          }
          rows={reviewRows(review.scope, text)}
        />
      )}
    </section>
  );
}

/** Presents the reviewed conditions that will govern the explicit scan. */
function reviewRows(scope: ScopeDraft, text: LocaleText) {
  const rows = [
    {
      label: text("ui.analysis.included_paths", "Included paths"),
      value: scope.includes.map((path) => path.display).join(", "),
    },
    {
      label: text("ui.analysis.boundary", "Boundary"),
      value:
        scope.boundary === "include_selected"
          ? text("ui.analysis.selected_mounts", "Selected mounts")
          : text("ui.analysis.current_volume", "Current volume"),
    },
  ];
  if (scope.includedMounts.length > 0) {
    rows.push({
      label: text("ui.analysis.included_mounts", "Included mounts"),
      value: scope.includedMounts.map((path) => path.display).join(", "),
    });
  }
  if (scope.exclusions.length > 0) {
    rows.push({
      label: text("ui.analysis.exclusions", "Exclusions"),
      value: scope.exclusions
        .map((rule) =>
          rule.kind === "path" ? rule.path.display : rule.pattern,
        )
        .join(", "),
    });
  }
  return rows;
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
  startLabel,
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
  const text = useLocaleText();
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
        {checkingDrop && (
          <Text>
            {text("ui.analysis.checking_drop", "Checking dropped folder…")}
          </Text>
        )}
      </LiveStatus>
      {reviewing && (
        <Progress label={text("ui.analysis.reviewing", "Reviewing scope")} />
      )}
      {starting && (
        <Progress label={text("ui.analysis.starting", "Starting scan")} />
      )}
      <div className="du-target-editor__actions">
        <Action isDisabled={working || running} onPress={onCancel}>
          {text("ui.analysis.cancel_selection", "Cancel selection")}
        </Action>
        <Action
          ref={reviewButton}
          icon="folder"
          variant={startEnabled ? "secondary" : "primary"}
          isDisabled={reviewDisabled}
          isPending={reviewPending}
          onPress={onReview}
        >
          {reviewing
            ? text("ui.analysis.reviewing_pending", "Reviewing scope…")
            : text("ui.analysis.review", "Review scope")}
        </Action>
        <Action
          icon="scan"
          variant={startEnabled ? "primary" : "secondary"}
          isDisabled={startDisabled}
          isPending={startPending}
          onPress={onStart}
        >
          {starting
            ? text("ui.analysis.starting_pending", "Starting scan…")
            : (startLabel ?? text("ui.analysis.start", "Start scan"))}
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
  const text = useLocaleText();
  const browse = version && onBrowse && (
    <Action
      ref={trigger ?? null}
      icon="folder"
      variant="primary"
      onPress={() => onBrowse(version)}
    >
      {text("ui.analysis.browse", "Browse folders")}
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
          {shownCommand === "pause"
            ? text("ui.analysis.pause", "Pause scan")
            : text("ui.analysis.resume", "Resume scan")}
        </Action>
      )}
      {showCancel && (
        <Action
          ref={cancel}
          isPending={!running || cancelling}
          onPress={() => onControl("cancel")}
        >
          {text("ui.analysis.cancel_scan", "Cancel scan")}
        </Action>
      )}
    </div>
  );
}
