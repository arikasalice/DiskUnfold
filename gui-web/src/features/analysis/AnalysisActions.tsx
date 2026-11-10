import { useLayoutEffect, useRef } from "react";
import type { Ref } from "react";
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

function noticeText(key: string): string {
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
  problem,
  problemId,
}: {
  review: ScopeReview | undefined;
  ready: boolean;
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
  if (ready)
    return review.notices.map((notice) => (
      <Notice key={notice}>{noticeText(notice)}</Notice>
    ));

  const messages = new Set(review.notices.map(noticeText));
  for (const target of review.targets) {
    if (target.access !== "accessible" && target.reasonKey)
      messages.add(noticeText(target.reasonKey));
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
  problem,
  problemId,
}: {
  review: ScopeReview | undefined;
  ready: boolean;
  problem: string | undefined;
  problemId: string;
}) {
  const feedback = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    feedback.current?.scrollIntoView({ block: "nearest" });
  }, [review, problem]);

  if (!review && !problem) return null;
  return (
    <section
      className="du-review-feedback"
      ref={feedback}
      aria-label={ready ? "Reviewed scope" : "Scope review problem"}
    >
      <ReviewNotices
        review={review}
        ready={ready}
        problem={problem}
        problemId={problemId}
      />
      {review && ready && (
        <Readout
          label="Scope paths and boundary"
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

export function ScopeActions({
  checkingDrop,
  reviewing,
  working,
  running,
  reviewEnabled,
  startEnabled,
  onReview,
  onStart,
}: {
  checkingDrop: boolean;
  reviewing: boolean;
  working: boolean;
  running: boolean;
  reviewEnabled: boolean;
  startEnabled: boolean;
  onReview: () => void;
  onStart: () => void;
}) {
  const blocked = reviewing || working || running;
  const starting = working && !running;
  return (
    <footer className="du-target-editor__footer">
      <LiveStatus>
        {checkingDrop && <Text>Checking dropped folder…</Text>}
      </LiveStatus>
      {reviewing && <Progress label="Reviewing scope" />}
      {starting && <Progress label="Starting scan" />}
      <div className="du-target-editor__actions">
        <Action
          icon="folder"
          variant={startEnabled ? "secondary" : "primary"}
          isDisabled={blocked || !reviewEnabled}
          isPending={checkingDrop && !blocked && reviewEnabled}
          onPress={onReview}
        >
          {reviewing ? "Reviewing scope…" : "Review scope"}
        </Action>
        <Action
          icon="scan"
          variant={startEnabled ? "primary" : "secondary"}
          isDisabled={blocked || !startEnabled}
          isPending={checkingDrop && !blocked && startEnabled}
          onPress={onStart}
        >
          {starting ? "Starting scan…" : "Start scan"}
        </Action>
      </div>
    </footer>
  );
}

export function ResultActions({
  version,
  onBrowse,
  trigger,
  running,
  state,
  pending,
  onControl,
}: {
  version: string | undefined;
  onBrowse: ((version: string) => void) | undefined;
  trigger: Ref<HTMLButtonElement> | undefined;
  running: boolean;
  state: JobState | undefined;
  pending: ScanControl | undefined;
  onControl: (command: ScanControl) => void;
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
  if (!browse && !running) return null;
  const command =
    state === "running"
      ? "pause"
      : state === "pause_requested" || state === "paused"
        ? "resume"
        : undefined;
  const cancelling = pending === "cancel" || state === "cancel_requested";
  return (
    <div className="du-result__actions">
      {browse}
      {running && command && (
        <Action
          isPending={pending === command}
          isDisabled={cancelling}
          onPress={() => onControl(command)}
        >
          {command === "pause" ? "Pause scan" : "Resume scan"}
        </Action>
      )}
      {running && (
        <Action isDisabled={cancelling} onPress={() => onControl("cancel")}>
          Cancel scan
        </Action>
      )}
    </div>
  );
}
