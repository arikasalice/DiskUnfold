import { useId, useLayoutEffect, useRef } from "react";
import type { RefObject } from "react";
import { useTimestampText } from "../../i18n/regional-format";
import { useRegionalFormat } from "../../i18n/regional-format";
import { useLocaleText } from "../../i18n/presentation";
import {
  Action,
  Heading,
  LiveStatus,
  Notice,
  Progress,
  Text,
} from "../../components";
import type {
  RecentScopeList,
  RecentScopeListRow,
  RecentScopeOpen,
  RecentScopeOpenTarget,
  ScopeDraft,
  ScopeReview,
} from "../../ipc";
import { useRecentScopes } from "./use-recent-scopes";
import { formatDate, PathValue, targetStatus } from "./RecentScopeDisplay";
import { revealConfirmGroup } from "./recent-confirm-scroll";

export type View = "closed" | "list" | "detail";
export function detailRow(
  detail: ReturnType<typeof useRecentScopes>["detail"],
): RecentScopeListRow | undefined {
  return "row" in detail ? detail.row : undefined;
}

export function focusTransition({
  view,
  previous,
  listHeading,
  detailHeading,
  rowButtons,
  returnedRow,
  scrollContainer,
  savedScrollTop,
  editorScrollTop,
  applied,
  path,
  applyFocus,
  pathFocus,
  opener,
}: {
  view: View;
  previous: View;
  listHeading: { current: HTMLDivElement | null };
  detailHeading: { current: HTMLDivElement | null };
  rowButtons: { current: Map<string, HTMLButtonElement> };
  returnedRow: string | undefined;
  scrollContainer: HTMLElement | null | undefined;
  savedScrollTop: number;
  editorScrollTop: number;
  applied: { current: boolean };
  path: { current: boolean };
  applyFocus: HTMLElement | null | undefined;
  pathFocus: HTMLElement | null | undefined;
  opener: HTMLButtonElement | null;
}) {
  if (view === "list") {
    if (previous === "detail") {
      if (scrollContainer) scrollContainer.scrollTop = savedScrollTop;
      const target =
        rowButtons.current.get(returnedRow ?? "") ?? listHeading.current;
      target?.focus({ preventScroll: true });
      target?.scrollIntoView({ block: "nearest", inline: "nearest" });
    } else {
      listHeading.current?.focus();
    }
  } else if (view === "detail") detailHeading.current?.focus();
  else {
    if (applied.current) {
      (applyFocus ?? opener)?.focus();
      applied.current = false;
    } else if (path.current) {
      (pathFocus ?? opener)?.focus();
      path.current = false;
    } else {
      opener?.focus({ preventScroll: true });
      if (scrollContainer) scrollContainer.scrollTop = editorScrollTop;
    }
  }
}

type RecentState = ReturnType<typeof useRecentScopes>;

export function RecentScopeListView({
  list,
  rowButtons,
  onRetry,
  onOpen,
}: {
  list: RecentState["list"];
  rowButtons: { current: Map<string, HTMLButtonElement> };
  onRetry: () => void;
  onOpen: (row: RecentScopeListRow) => void;
}) {
  const text = useLocaleText();
  return (
    <div className="du-recent-scopes__body">
      {list.kind === "idle" && (
        <Action isPending onPress={onRetry}>
          {text("ui.analysis.loading_recent_ranges", "Loading recent ranges…")}
        </Action>
      )}
      {list.kind === "loading" && (
        <LoadingMessage
          message={text(
            "ui.analysis.loading_recent_ranges_full",
            "Loading recent analysis ranges…",
          )}
          label={text(
            "ui.analysis.loading_recent_ranges_label",
            "Loading recent analysis ranges",
          )}
        />
      )}
      {list.kind === "error" && (
        <Notice tone="error">
          <div className="du-recent-scopes__error">
            <Text>
              {text(
                "ui.analysis.recent_ranges_load_failed",
                "Recent analysis ranges could not be loaded.",
              )}
            </Text>
            <Action onPress={onRetry}>
              {text("ui.analysis.try_again", "Try again")}
            </Action>
          </div>
        </Notice>
      )}
      {list.kind === "ready" && list.value.rows.length === 0 && (
        <Notice tone="info">
          {text(
            "ui.analysis.no_recent_ranges",
            "There are no recent analysis ranges. Return to folder selection to choose a path.",
          )}
        </Notice>
      )}
      {list.kind === "ready" && list.value.rows.length > 0 && (
        <RecentScopeRows
          rows={list.value.rows}
          rowButtons={rowButtons}
          onOpen={onOpen}
        />
      )}
    </div>
  );
}

function RecentScopeRows({
  rows,
  rowButtons,
  onOpen,
}: {
  rows: RecentScopeList["rows"];
  rowButtons: { current: Map<string, HTMLButtonElement> };
  onOpen: (row: RecentScopeListRow) => void;
}) {
  const timestamp = useTimestampText();
  const text = useLocaleText();
  const regional = useRegionalFormat();
  return (
    <div className="du-recent-scopes__rows" role="list">
      {rows.map((row, index) => (
        <div key={row.recentScopeRef} role="listitem">
          <div className="du-recent-scopes__summary">
            <Text>
              {text("ui.analysis.recent_range_number", "Range {number}", {
                number: BigInt(index + 1),
              })}
            </Text>
            <Text kind="muted">
              {timestamp(Date.parse(row.lastUsedAt), () =>
                formatDate(row.lastUsedAt, regional),
              )}{" "}
              ·{" "}
              {text("ui.analysis.recent_location_count", "{count} locations", {
                count: BigInt(row.includeCount),
              })}
            </Text>
          </div>
          <Action
            ref={(button) => {
              if (button) rowButtons.current.set(row.recentScopeRef, button);
              else rowButtons.current.delete(row.recentScopeRef);
            }}
            onPress={() => onOpen(row)}
          >
            {text("ui.analysis.review_range_number", "Review range {number}", {
              number: BigInt(index + 1),
            })}
          </Action>
        </div>
      ))}
    </div>
  );
}

export function RecentScopeDetailView({
  detail,
  isRequestSeqCurrent,
  hasDraftEdits,
  confirmReplace,
  scrollContainerRef,
  onConfirmReplace,
  onRetry,
  onRefresh,
  onBack,
  onChoosePath,
  onApply,
}: {
  detail: RecentState["detail"];
  isRequestSeqCurrent: (requestSeq: number) => boolean;
  hasDraftEdits: boolean;
  confirmReplace: boolean;
  scrollContainerRef: RefObject<HTMLElement | null> | undefined;
  onConfirmReplace: (confirm: boolean) => void;
  onRetry: (row: RecentScopeListRow) => void;
  onRefresh: () => void;
  onBack: (row?: RecentScopeListRow) => void;
  onChoosePath: () => void;
  onApply: (draft: ScopeDraft, review: ScopeReview) => void;
}) {
  const active = detail.kind === "ready" ? detail.value : undefined;
  const review = active?.review ?? undefined;
  const reviewCurrent = Boolean(
    review && isRequestSeqCurrent(review.requestSeq),
  );
  const canApply = active ? isApplicable(active, review, reviewCurrent) : false;
  return (
    <div className="du-recent-scopes__body">
      <DetailFeedback
        detail={detail}
        onRetry={onRetry}
        onRefresh={onRefresh}
        onBack={onBack}
      />
      {active && (
        <>
          <SavedConditions draft={active.draft} />
          <CurrentPathStatus
            value={active}
            reviewCurrent={reviewCurrent}
            onCheckAgain={() => detail.kind === "ready" && onRetry(detail.row)}
          />
          <RecentScopeApplyActions
            value={active}
            review={review}
            canApply={canApply}
            hasDraftEdits={hasDraftEdits}
            confirmReplace={confirmReplace}
            scrollContainerRef={scrollContainerRef}
            onConfirmReplace={onConfirmReplace}
            onChoosePath={onChoosePath}
            onApply={onApply}
          />
        </>
      )}
    </div>
  );
}

function LoadingMessage({
  message,
  label,
}: {
  message: string;
  label: string;
}) {
  return (
    <div className="du-recent-scopes__loading">
      <LiveStatus>
        <Text>{message}</Text>
      </LiveStatus>
      <Progress label={label} />
    </div>
  );
}

function DetailFeedback({
  detail,
  onRetry,
  onRefresh,
  onBack,
}: {
  detail: RecentState["detail"];
  onRetry: (row: RecentScopeListRow) => void;
  onRefresh: () => void;
  onBack: (row?: RecentScopeListRow) => void;
}) {
  const text = useLocaleText();
  if (detail.kind === "loading")
    return (
      <LoadingMessage
        message={text(
          "ui.analysis.checking_saved_paths",
          "Checking saved paths and access…",
        )}
        label={text(
          "ui.analysis.checking_saved_paths_label",
          "Checking saved paths and access",
        )}
      />
    );
  if (detail.kind !== "error") return null;
  return (
    <Notice tone="error">
      <div className="du-recent-scopes__error">
        <Text>{detailMessage(detail.message, text)}</Text>
        {detail.missing ? (
          <Action onPress={onRefresh}>
            {text("ui.analysis.refresh_recent_ranges", "Refresh list")}
          </Action>
        ) : (
          <Action onPress={() => onRetry(detail.row)}>
            {text("ui.analysis.try_again", "Try again")}
          </Action>
        )}
        <Action variant="quiet" onPress={() => onBack(detail.row)}>
          {text("ui.analysis.back_to_recent_ranges", "Back to recent ranges")}
        </Action>
      </div>
    </Notice>
  );
}

function detailMessage(
  message: string,
  text: ReturnType<typeof useLocaleText>,
) {
  switch (message) {
    case "The scope changed. Check the saved range again.":
      return text(
        "ui.analysis.recent_scope_changed",
        "The scope changed. Check the saved range again.",
      );
    case "The scope review changed. Open the saved range again.":
      return text(
        "ui.analysis.recent_scope_review_changed",
        "The scope review changed. Open the saved range again.",
      );
    case "This saved range is no longer available. Refresh the list and choose another range.":
      return text(
        "ui.analysis.recent_range_missing",
        "This saved range is no longer available. Refresh the list and choose another range.",
      );
    case "This saved range could not be opened. It may be damaged or from an unsupported version.":
      return text(
        "ui.analysis.recent_range_open_failed",
        "This saved range could not be opened. It may be damaged or from an unsupported version.",
      );
    default:
      return message;
  }
}

function SavedConditions({ draft }: { draft: ScopeDraft }) {
  const text = useLocaleText();
  return (
    <section
      aria-label={text("ui.analysis.saved_conditions", "Saved conditions")}
    >
      <Heading level={3}>
        {text("ui.analysis.saved_conditions", "Saved conditions")}
      </Heading>
      <div className="du-recent-scopes__conditions">
        <Text kind="muted">
          {text("ui.analysis.included_locations", "Included locations")}
        </Text>
        <div>
          {draft.includes.map((path, index) => (
            <PathValue
              key={`${path.raw}-${index}`}
              value={path.display}
              lossy={path.lossy}
            />
          ))}
        </div>
        <Text kind="muted">{text("ui.analysis.exclusions", "Exclusions")}</Text>
        <div>
          {draft.exclusions.length === 0
            ? text("ui.analysis.none", "None")
            : draft.exclusions.map((item, index) => (
                <PathValue
                  key={index}
                  value={
                    item.kind === "path" ? item.path.display : item.pattern
                  }
                  lossy={item.kind === "path" && item.path.lossy}
                />
              ))}
        </div>
        <Text kind="muted">
          {text("ui.analysis.volume_boundary", "Volume boundary")}
        </Text>
        <div>
          <Text>
            {draft.boundary === "current_volume"
              ? text(
                  "ui.analysis.stay_within_current_volume",
                  "Stay within each current volume",
                )
              : text(
                  "ui.analysis.include_mounted_volumes",
                  "Include selected mounted volumes",
                )}
          </Text>
        </div>
        <Text kind="muted">
          {text("ui.analysis.selected_mounts", "Selected mounts")}
        </Text>
        <div>
          {draft.includedMounts.length === 0
            ? text("ui.analysis.none", "None")
            : draft.includedMounts.map((path, index) => (
                <PathValue
                  key={`${path.raw}-${index}`}
                  value={path.display}
                  lossy={path.lossy}
                />
              ))}
        </div>
      </div>
    </section>
  );
}

function CurrentPathStatus({
  value,
  reviewCurrent,
  onCheckAgain,
}: {
  value: RecentScopeOpen;
  reviewCurrent: boolean;
  onCheckAgain: () => void;
}) {
  const text = useLocaleText();
  const unresolvedCount = value.targets.filter(
    (target) => target.state !== "matched",
  ).length;
  return (
    <section
      aria-label={text(
        "ui.analysis.current_path_status",
        "Current path status",
      )}
    >
      <Heading level={3}>
        {text("ui.analysis.current_path_status", "Current path status")}
      </Heading>
      <div className="du-recent-scopes__targets" role="list">
        {value.targets.map((target, index) => (
          <CurrentPathRow key={`${target.path.raw}-${index}`} target={target} />
        ))}
      </div>
      {value.review === null &&
        value.targets.every((t) => t.state === "matched") && (
          <>
            <Notice tone="info">
              {text(
                "ui.analysis.recent_paths_access_unreviewed",
                "Current paths and access could not be reviewed. Check the saved range again or choose the paths and review them before using this range.",
              )}
            </Notice>
            <Action onPress={onCheckAgain}>
              {text("ui.analysis.check_paths_again", "Check paths again")}
            </Action>
          </>
        )}
      {!reviewCurrent && value.review !== null && (
        <>
          <Notice tone="info">
            {text(
              "ui.analysis.recent_editor_scope_changed",
              "The editor scope changed. Check this saved range again before using it.",
            )}
          </Notice>
          <Action onPress={onCheckAgain}>
            {text("ui.analysis.check_paths_again", "Check paths again")}
          </Action>
        </>
      )}
      {unresolvedCount > 0 && (
        <Notice tone="info">
          {text(
            "ui.analysis.recent_unresolved_paths",
            "{count} of {total} locations need attention. Choose a path again for unresolved locations. Saved paths are not applied automatically.",
            {
              count: BigInt(unresolvedCount),
              total: BigInt(value.targets.length),
            },
          )}
        </Notice>
      )}
    </section>
  );
}

function CurrentPathRow({ target }: { target: RecentScopeOpenTarget }) {
  const text = useLocaleText();
  return (
    <div role="listitem">
      <PathValue value={target.path.display} lossy={target.path.lossy} />
      <Text>{targetStatus(target, text)}</Text>
      {target.state === "moved" && target.proposedEntry && (
        <div className="du-recent-scopes__proposed">
          <Text kind="muted">
            {text("ui.analysis.current_volume_entry", "Current volume entry:")}
          </Text>
          <PathValue
            value={target.proposedEntry.display}
            lossy={target.proposedEntry.lossy}
          />
        </div>
      )}
    </div>
  );
}

function RecentScopeApplyActions({
  value,
  review,
  canApply,
  hasDraftEdits,
  confirmReplace,
  scrollContainerRef,
  onConfirmReplace,
  onChoosePath,
  onApply,
}: {
  value: RecentScopeOpen;
  review: ScopeReview | undefined;
  canApply: boolean;
  hasDraftEdits: boolean;
  confirmReplace: boolean;
  scrollContainerRef: RefObject<HTMLElement | null> | undefined;
  onConfirmReplace: (confirm: boolean) => void;
  onChoosePath: () => void;
  onApply: (draft: ScopeDraft, review: ScopeReview) => void;
}) {
  const text = useLocaleText();
  const confirmButton = useRef<HTMLButtonElement>(null);
  const confirmGroup = useRef<HTMLDivElement>(null);
  const confirmDescription = useRef<HTMLDivElement>(null);
  const useButton = useRef<HTMLButtonElement>(null);
  const descriptionId = useId();
  const pendingFocus = useRef(false);
  useLayoutEffect(() => {
    if (!pendingFocus.current) return;
    const target = confirmReplace ? confirmButton.current : useButton.current;
    if (confirmReplace && confirmGroup.current && scrollContainerRef?.current) {
      const oversized = revealConfirmGroup(
        scrollContainerRef.current,
        confirmGroup.current,
      );
      (oversized ? confirmDescription.current : target)?.focus({
        preventScroll: true,
      });
    } else {
      target?.focus({ preventScroll: true });
      target?.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
    pendingFocus.current = false;
  }, [confirmReplace, scrollContainerRef]);
  function toggleConfirm(value: boolean) {
    pendingFocus.current = true;
    onConfirmReplace(value);
  }
  if (confirmReplace && review)
    return (
      <div
        ref={confirmGroup}
        className="du-recent-scopes__confirm"
        role="group"
        aria-label={text(
          "ui.analysis.replace_current_scope",
          "Replace current scope",
        )}
      >
        <div
          ref={confirmDescription}
          id={descriptionId}
          tabIndex={-1}
          className="du-recent-scopes__confirm-description"
        >
          <Text>
            {text(
              "ui.analysis.replace_scope_confirmation",
              "Replace the current folder paths, exclusions, boundary, and selected mounts with these saved conditions?",
            )}
          </Text>
        </div>
        <Action
          ref={confirmButton}
          aria-describedby={descriptionId}
          variant="primary"
          onPress={() => onApply(value.draft, review)}
        >
          {text("ui.analysis.replace_current_scope", "Replace current scope")}
        </Action>
        <Action
          aria-describedby={descriptionId}
          variant="quiet"
          onPress={() => toggleConfirm(false)}
        >
          {text("ui.analysis.keep_editing", "Keep editing")}
        </Action>
      </div>
    );
  return (
    <div className="du-recent-scopes__actions">
      {canApply && review && (
        <Action
          ref={useButton}
          variant="primary"
          onPress={() =>
            hasDraftEdits ? toggleConfirm(true) : onApply(value.draft, review)
          }
        >
          {text("ui.analysis.use_recent_range", "Use this range")}
        </Action>
      )}
      {!canApply && (
        <Action onPress={onChoosePath}>
          {text("ui.analysis.choose_path_again", "Choose a path again")}
        </Action>
      )}
    </div>
  );
}

function isApplicable(
  value: RecentScopeOpen,
  review: ScopeReview | undefined,
  reviewCurrent: boolean,
): boolean {
  return Boolean(
    review &&
    reviewCurrent &&
    value.targets.length > 0 &&
    value.targets.every((target) => target.state === "matched") &&
    review.scope.includes.length > 0 &&
    review.targets.length === review.scope.includes.length &&
    review.targets.every((target) => target.access === "accessible"),
  );
}
