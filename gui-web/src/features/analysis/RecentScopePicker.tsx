import { useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import { Action, Heading } from "../../components";
import type {
  AnalysisClient,
  RecentScopeListRow,
  ScopeDraft,
  ScopeReview,
} from "../../ipc";
import { useRecentScopes } from "./use-recent-scopes";
import {
  detailRow,
  focusTransition,
  RecentScopeDetailView,
  RecentScopeListView,
} from "./RecentScopeViews";
import "./recent-scopes.css";
import type { View } from "./RecentScopeViews";

/** Provides an inline review flow for reopening one saved analysis range. */
export function RecentScopePicker({
  client,
  currentDraft,
  nextRequestSeq,
  isRequestSeqCurrent,
  onApply,
  onChoosePath,
  onOpenChange,
  isDisabled = false,
  openerRef,
  pathFocusRef,
  applyFocusRef,
  scrollContainerRef,
}: {
  client: AnalysisClient;
  currentDraft: ScopeDraft | undefined;
  /** Must share the editor's monotonically increasing scope-review sequence. */
  nextRequestSeq: () => number;
  /** Rejects saved-range results when the host editor has moved to another scope generation. */
  isRequestSeqCurrent: (requestSeq: number) => boolean;
  /** Apply the unmodified recent draft/review to editor state; never starts a scan. */
  onApply: (draft: ScopeDraft, review: ScopeReview) => void;
  onChoosePath: () => void;
  onOpenChange?: (open: boolean) => void;
  isDisabled?: boolean;
  openerRef?: RefObject<HTMLButtonElement | null>;
  pathFocusRef?: RefObject<HTMLElement | null>;
  /** Host focus target for the editor scope summary or first editable path. */
  applyFocusRef?: RefObject<HTMLElement | null>;
  /** Host's existing editor scroll region; no nested scroll area is created. */
  scrollContainerRef?: RefObject<HTMLElement | null>;
}) {
  const [view, setView] = useState<View>("closed");
  const [confirmReplace, setConfirmReplace] = useState(false);
  const pickerOpener = useRef<HTMLButtonElement>(null);
  const listHeading = useRef<HTMLDivElement>(null);
  const detailHeading = useRef<HTMLDivElement>(null);
  const rowButtons = useRef(new Map<string, HTMLButtonElement>());
  const returnedRow = useRef<string | undefined>(undefined);
  const focusPathAfterClose = useRef(false);
  const focusAppliedAfterClose = useRef(false);
  const savedScrollTop = useRef(0);
  const editorScrollTop = useRef(0);
  const previousView = useRef<View>("closed");
  const { list, detail, loadList, openRow, invalidate } = useRecentScopes(
    client,
    nextRequestSeq,
    isRequestSeqCurrent,
  );

  useLayoutEffect(() => {
    if (previousView.current !== view) {
      focusTransition({
        view,
        previous: previousView.current,
        listHeading,
        detailHeading,
        rowButtons,
        returnedRow: returnedRow.current,
        scrollContainer: scrollContainerRef?.current,
        savedScrollTop: savedScrollTop.current,
        editorScrollTop: editorScrollTop.current,
        applied: focusAppliedAfterClose,
        path: focusPathAfterClose,
        applyFocus: applyFocusRef?.current,
        pathFocus: pathFocusRef?.current,
        opener: openerRef?.current ?? pickerOpener.current,
      });
      previousView.current = view;
    }
  }, [applyFocusRef, openerRef, pathFocusRef, scrollContainerRef, view]);

  function openList() {
    if (view === "closed")
      editorScrollTop.current = scrollContainerRef?.current?.scrollTop ?? 0;
    onOpenChange?.(true);
    setConfirmReplace(false);
    setView("list");
    void loadList();
  }

  function close() {
    invalidate();
    onOpenChange?.(false);
    setConfirmReplace(false);
    setView("closed");
  }

  function backToList(row?: RecentScopeListRow) {
    returnedRow.current = row?.recentScopeRef;
    invalidate();
    setConfirmReplace(false);
    setView("list");
  }

  function choosePathAgain() {
    focusPathAfterClose.current = true;
    close();
    onChoosePath();
  }

  function acceptRange(draft: ScopeDraft, review: ScopeReview) {
    if (!isRequestSeqCurrent(review.requestSeq)) return;
    focusAppliedAfterClose.current = true;
    onApply(draft, review);
    close();
  }

  return (
    <section className="du-recent-scopes" aria-label="Recent analysis ranges">
      <Action
        ref={pickerOpener}
        variant="quiet"
        isDisabled={isDisabled}
        aria-expanded={view !== "closed"}
        onPress={openList}
      >
        Recent analysis ranges
      </Action>
      {view !== "closed" && (
        <section
          className="du-recent-scopes__panel"
          aria-label="Recent analysis ranges"
        >
          <header className="du-recent-scopes__header">
            {view === "list" ? (
              <div
                ref={listHeading}
                tabIndex={-1}
                className="du-recent-scopes__focus-heading"
              >
                <Heading level={3}>Recent analysis ranges</Heading>
              </div>
            ) : (
              <div
                ref={detailHeading}
                tabIndex={-1}
                className="du-recent-scopes__focus-heading"
              >
                <Heading level={3}>Review saved range</Heading>
              </div>
            )}
            {view === "detail" && (
              <Action
                variant="quiet"
                onPress={() => backToList(detailRow(detail))}
              >
                Back to recent ranges
              </Action>
            )}
            <Action variant="quiet" onPress={close}>
              Close recent ranges
            </Action>
          </header>

          {view === "list" && (
            <RecentScopeListView
              list={list}
              rowButtons={rowButtons}
              onRetry={() => void loadList()}
              onOpen={(row) => {
                if (scrollContainerRef?.current)
                  savedScrollTop.current = scrollContainerRef.current.scrollTop;
                returnedRow.current = row.recentScopeRef;
                setView("detail");
                void openRow(row);
              }}
            />
          )}

          {view === "detail" && (
            <RecentScopeDetailView
              detail={detail}
              isRequestSeqCurrent={isRequestSeqCurrent}
              currentDraft={currentDraft}
              confirmReplace={confirmReplace}
              onConfirmReplace={setConfirmReplace}
              onRetry={(row) => void openRow(row)}
              onRefresh={() => {
                setView("list");
                void loadList();
              }}
              onBack={backToList}
              onChoosePath={choosePathAgain}
              onApply={acceptRange}
            />
          )}
        </section>
      )}
    </section>
  );
}
