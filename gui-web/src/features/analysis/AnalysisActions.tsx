import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, Ref, RefObject } from "react";
import {
  Action,
  LiveStatus,
  MiddleEllipsisText,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
import { scopeNoticeText } from "../../ipc";
import type { JobState, OsPath, ScopeDraft, ScopeReview } from "../../ipc";
import type { ScanControl } from "../../state/analysis-scan";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { ExclusionRules } from "./ExclusionRules";

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
    return review.notices
      .filter((notice) => !isVolumeGroupNotice(notice))
      .map((notice) => (
        <Notice key={notice}>{scopeNoticeText(notice, text)}</Notice>
      ));

  const messages = new Set(
    review.notices
      .filter((notice) => !isVolumeGroupNotice(notice))
      .map((notice) => scopeNoticeText(notice, text)),
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
  const lines = [...messages];
  return (
    <Notice tone="error" id={problemId}>
      {lines.map((message) => (
        <Text key={message}>{message}</Text>
      ))}
    </Notice>
  );
}

function isVolumeGroupNotice(notice: string) {
  return (
    notice === "scope.volume_group_merged" ||
    notice === "scope.volume_group_unverified"
  );
}

/** Selects the supported recovery action from the reviewed notice and suggestion. */
function chooseFolderAction(
  review: ScopeReview,
  onChooseFolder: (suggestedInclude?: OsPath) => void,
  text: LocaleText,
) {
  const suggestedInclude = review.suggestedInclude;
  if (
    suggestedInclude &&
    review.notices.includes("scope.volume_group_unverified")
  ) {
    const marker = "\uE000path\uE001";
    const label = text(
      "ui.analysis.analyze_home_folder",
      "Analyze home folder ({path})",
      { path: marker },
    );
    const [prefix, suffix = ""] = label.split(marker);
    return (
      <Action onPress={() => onChooseFolder(suggestedInclude)}>
        {prefix}
        <MiddleEllipsisText text={suggestedInclude.display} />
        {suffix}
      </Action>
    );
  }
  return (
    <Action onPress={() => onChooseFolder()}>
      {text("ui.analysis.choose_folder", "Choose a folder")}
    </Action>
  );
}

function VolumeGroupNotice({
  review,
  onChooseFolder,
}: {
  review: ScopeReview;
  onChooseFolder: (suggestedInclude?: OsPath) => void;
}) {
  const text = useLocaleText();
  const merged = review.notices.includes("scope.volume_group_merged");
  const unverified = review.notices.includes("scope.volume_group_unverified");
  if (!merged && !unverified) return null;
  return (
    <Notice tone={unverified ? "warning" : "info"}>
      {unverified
        ? text(
            "ui.analysis.volume_group_unverified",
            "The data volume couldn't be confirmed, so only the system volume is analyzed. User files aren't included.",
          )
        : text(
            "ui.analysis.volume_group_merged",
            "The system and data volumes are analyzed together. Each file is counted once.",
          )}
      {unverified && chooseFolderAction(review, onChooseFolder, text)}
    </Notice>
  );
}

export function ResultGroupNotice({ unverified }: { unverified: boolean }) {
  const text = useLocaleText();
  if (!unverified) return null;
  return (
    <Notice tone="warning">
      {text(
        "ui.analysis.volume_group_unverified",
        "The data volume couldn't be confirmed, so only the system volume is analyzed. User files aren't included.",
      )}
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
  onChooseFolder,
}: {
  review: ScopeReview | undefined;
  ready: boolean;
  current: boolean;
  latestAccepted: boolean;
  problem: string | undefined;
  problemId: string;
  onChooseFolder: (suggestedInclude?: OsPath) => void;
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
      {review && current && (
        <VolumeGroupNotice review={review} onChooseFolder={onChooseFolder} />
      )}
    </section>
  );
}

/** Presents the reviewed conditions that will govern the explicit scan. */
function reviewRows(scope: ScopeDraft, text: LocaleText) {
  const rows: { label: string; value: ReactNode }[] = [
    {
      label: text("ui.analysis.included_paths", "Included paths"),
      value: scope.includes.map((path) => path.display).join(", "),
    },
    {
      label: text("ui.analysis.boundary", "Boundary"),
      value:
        scope.boundary === "include_selected"
          ? text("ui.analysis.selected_mounts", "Selected mounts")
          : text("ui.analysis.current_volume", "Current drive"),
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
      value: <ExclusionRules rules={scope.exclusions} />,
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
        <Progress label={text("ui.analysis.starting", "Starting analysis")} />
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
            ? text("ui.analysis.starting_pending", "Starting analysis…")
            : (startLabel ?? text("ui.analysis.start", "Start analysis"))}
        </Action>
      </div>
    </footer>
  );
}

/**
 * Keeps the focus in the result when a scan control that holds it goes away
 * because the scan ended or was cancelled: "Browse folders" in the same row
 * takes it, or the screen title when the row has no such action. Focus that
 * is elsewhere stays where it is.
 */
function useResultControlFocus(
  running: boolean,
  command: "pause" | "resume" | undefined,
  browse: RefObject<HTMLButtonElement | null>,
  title: RefObject<HTMLHeadingElement | null>,
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
      (browse.current ?? title.current)?.focus({ preventScroll: true });
    setPrevious({ running, command });
  }, [browse, command, previous, running, title]);

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
  title,
}: {
  version: string | undefined;
  onBrowse: ((version: string) => void) | undefined;
  trigger: RefObject<HTMLButtonElement | null> | undefined;
  running: boolean;
  state: JobState | undefined;
  pending: ScanControl | undefined;
  onControl: (command: ScanControl) => void;
  /** The screen title, which takes the focus when the row has no action for it. */
  title: RefObject<HTMLHeadingElement | null>;
}) {
  const text = useLocaleText();
  const ownBrowse = useRef<HTMLButtonElement>(null);
  const browseButton = trigger ?? ownBrowse;
  const browse = version && onBrowse && (
    <Action
      ref={browseButton}
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
    browseButton,
    title,
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
