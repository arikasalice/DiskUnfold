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
  applied: { current: boolean };
  path: { current: boolean };
  applyFocus: HTMLElement | null | undefined;
  pathFocus: HTMLElement | null | undefined;
  opener: HTMLButtonElement | null;
}) {
  if (view === "list" && previous === "closed") listHeading.current?.focus();
  else if (view === "list" && previous === "detail") {
    if (scrollContainer) scrollContainer.scrollTop = savedScrollTop;
    (rowButtons.current.get(returnedRow ?? "") ?? listHeading.current)?.focus();
  } else if (view === "detail") detailHeading.current?.focus();
  else if (view === "closed") {
    if (applied.current) {
      (applyFocus ?? opener)?.focus();
      applied.current = false;
    } else if (path.current) {
      (pathFocus ?? opener)?.focus();
      path.current = false;
    } else opener?.focus();
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
  return (
    <div className="du-recent-scopes__body">
      {list.kind === "idle" && (
        <Action isPending onPress={onRetry}>
          Loading recent ranges…
        </Action>
      )}
      {list.kind === "loading" && (
        <LoadingMessage
          message="Loading recent analysis ranges…"
          label="Loading recent analysis ranges"
        />
      )}
      {list.kind === "error" && (
        <Notice tone="error">
          <div className="du-recent-scopes__error">
            <Text>Recent analysis ranges could not be loaded.</Text>
            <Action onPress={onRetry}>Try again</Action>
          </div>
        </Notice>
      )}
      {list.kind === "ready" && list.value.rows.length === 0 && (
        <Notice tone="info">
          There are no recent analysis ranges. Return to folder selection to
          choose a path.
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
  return (
    <div className="du-recent-scopes__rows" role="list">
      {rows.map((row, index) => (
        <div key={row.recentScopeRef} role="listitem">
          <div className="du-recent-scopes__summary">
            <Text>{`Range ${index + 1}`}</Text>
            <Text kind="muted">
              {formatDate(row.lastUsedAt)} · {row.includeCount}{" "}
              {row.includeCount === 1 ? "location" : "locations"}
            </Text>
          </div>
          <Action
            ref={(button) => {
              if (button) rowButtons.current.set(row.recentScopeRef, button);
              else rowButtons.current.delete(row.recentScopeRef);
            }}
            onPress={() => onOpen(row)}
          >
            Review range {index + 1}
          </Action>
        </div>
      ))}
    </div>
  );
}

export function RecentScopeDetailView({
  detail,
  isRequestSeqCurrent,
  currentDraft,
  confirmReplace,
  onConfirmReplace,
  onRetry,
  onRefresh,
  onBack,
  onChoosePath,
  onApply,
}: {
  detail: RecentState["detail"];
  isRequestSeqCurrent: (requestSeq: number) => boolean;
  currentDraft: ScopeDraft | undefined;
  confirmReplace: boolean;
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
            currentDraft={currentDraft}
            confirmReplace={confirmReplace}
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
  if (detail.kind === "loading")
    return (
      <LoadingMessage
        message="Checking saved paths and access…"
        label="Checking saved paths and access"
      />
    );
  if (detail.kind !== "error") return null;
  return (
    <Notice tone="error">
      <div className="du-recent-scopes__error">
        <Text>{detail.message}</Text>
        {detail.missing ? (
          <Action onPress={onRefresh}>Refresh list</Action>
        ) : (
          <Action onPress={() => onRetry(detail.row)}>Try again</Action>
        )}
        <Action variant="quiet" onPress={() => onBack(detail.row)}>
          Back to recent ranges
        </Action>
      </div>
    </Notice>
  );
}

function SavedConditions({ draft }: { draft: ScopeDraft }) {
  return (
    <section aria-label="Saved conditions">
      <Heading level={3}>Saved conditions</Heading>
      <div className="du-recent-scopes__conditions">
        <Text kind="muted">Included locations</Text>
        <div>
          {draft.includes.map((path, index) => (
            <PathValue
              key={`${path.raw}-${index}`}
              value={path.display}
              lossy={path.lossy}
            />
          ))}
        </div>
        <Text kind="muted">Exclusions</Text>
        <div>
          {draft.exclusions.length === 0
            ? "None"
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
        <Text kind="muted">Volume boundary</Text>
        <div>
          <Text>
            {draft.boundary === "current_volume"
              ? "Stay within each current volume"
              : "Include selected mounted volumes"}
          </Text>
        </div>
        <Text kind="muted">Selected mounts</Text>
        <div>
          {draft.includedMounts.length === 0
            ? "None"
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
  const unresolvedCount = value.targets.filter(
    (target) => target.state !== "matched",
  ).length;
  return (
    <section aria-label="Current path status">
      <Heading level={3}>Current path status</Heading>
      <div className="du-recent-scopes__targets" role="list">
        {value.targets.map((target, index) => (
          <CurrentPathRow key={`${target.path.raw}-${index}`} target={target} />
        ))}
      </div>
      {value.review === null &&
        value.targets.every((t) => t.state === "matched") && (
          <>
            <Notice tone="info">
              Current paths and access could not be reviewed. Check the saved
              range again or choose the paths and review them before using this
              range.
            </Notice>
            <Action onPress={onCheckAgain}>Check paths again</Action>
          </>
        )}
      {!reviewCurrent && value.review !== null && (
        <>
          <Notice tone="info">
            The editor scope changed. Check this saved range again before using
            it.
          </Notice>
          <Action onPress={onCheckAgain}>Check paths again</Action>
        </>
      )}
      {unresolvedCount > 0 && (
        <Notice tone="info">
          {`${unresolvedCount} of ${value.targets.length} locations need attention. Choose a path again for unresolved locations. Saved paths are not applied automatically.`}
        </Notice>
      )}
    </section>
  );
}

function CurrentPathRow({ target }: { target: RecentScopeOpenTarget }) {
  return (
    <div role="listitem">
      <PathValue value={target.path.display} lossy={target.path.lossy} />
      <Text>{targetStatus(target)}</Text>
      {target.state === "moved" && target.proposedEntry && (
        <div className="du-recent-scopes__proposed">
          <Text kind="muted">Current volume entry:</Text>
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
  currentDraft,
  confirmReplace,
  onConfirmReplace,
  onChoosePath,
  onApply,
}: {
  value: RecentScopeOpen;
  review: ScopeReview | undefined;
  canApply: boolean;
  currentDraft: ScopeDraft | undefined;
  confirmReplace: boolean;
  onConfirmReplace: (confirm: boolean) => void;
  onChoosePath: () => void;
  onApply: (draft: ScopeDraft, review: ScopeReview) => void;
}) {
  if (confirmReplace && review)
    return (
      <div
        className="du-recent-scopes__confirm"
        role="group"
        aria-label="Replace current scope"
      >
        <Text>
          Replace the current folder paths, exclusions, boundary, and selected
          mounts with these saved conditions?
        </Text>
        <Action variant="primary" onPress={() => onApply(value.draft, review)}>
          Replace current scope
        </Action>
        <Action variant="quiet" onPress={() => onConfirmReplace(false)}>
          Keep editing
        </Action>
      </div>
    );
  return (
    <div className="du-recent-scopes__actions">
      {canApply && review && (
        <Action
          variant="primary"
          onPress={() =>
            currentDraft ? onConfirmReplace(true) : onApply(value.draft, review)
          }
        >
          Use this range
        </Action>
      )}
      {!canApply && <Action onPress={onChoosePath}>Choose a path again</Action>}
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
