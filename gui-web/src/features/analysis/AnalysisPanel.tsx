import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { Ref } from "react";
import { Action, Heading, Text } from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  OsPath,
  ScanStart,
  ScopeDraft,
  ScopeReview,
} from "../../ipc";
import { useScanResult } from "../../state/analysis-scan";
import { useTargets } from "../../state/analysis-targets";
import {
  ResultActions,
  ScopeActions,
  ScopeReviewFeedback,
  scopeNoticeText,
} from "./AnalysisActions";
import { ScanOutput } from "./AnalysisViews";
import { RecentScopePicker } from "./RecentScopePicker";
import { TargetPicker } from "./TargetPicker";
import type { SizeDisplay } from "../../i18n/bytes";
import type { SizeBasisFilter } from "./space-format";
import type { AnalysisDisplayPreferences } from "./use-display-preferences";

function errorText(error: unknown): string {
  if (error instanceof AnalysisError && error.code === "STALE_VERSION") {
    return "The target or result changed. Review the scope again.";
  }
  return error instanceof Error ? error.message : "The operation failed";
}

function reviewErrorText(error: unknown): string {
  if (error instanceof AnalysisError) {
    switch (error.reasonKey) {
      case "scope.target_unavailable":
        return "Check that every selected path is an accessible local folder, then review the scope again.";
      case "path.unresolvable":
        return "Enter a valid local folder path, then review the scope again.";
    }
  }
  return errorText(error);
}

function rejectedReviewText(review: ScopeReview): string {
  const reason = review.targets.find(
    (target) => target.access !== "accessible" && target.reasonKey,
  )?.reasonKey;
  if (reason === "path.unresolvable")
    return "Enter a valid local folder path, then review the scope again.";
  if (reason === "scope.target_unavailable")
    return "Check that every selected path is an accessible local folder, then review the scope again.";
  if (reason) return scopeNoticeText(reason);
  if (review.targets.some((target) => target.access === "denied"))
    return scopeNoticeText("scan.path_denied");
  return "One or more selected paths could not be verified. Review all paths before starting.";
}

function draft(paths: OsPath[], saved?: ScopeDraft): ScopeDraft {
  return {
    includes: paths,
    exclusions: saved?.exclusions ?? [],
    boundary: saved?.boundary ?? "current_volume",
    includedMounts: saved?.includedMounts ?? [],
    source: "gui",
  };
}

/** Keeps the original request bound to the fingerprint returned for it. */
type ReviewedScope = {
  input: ScopeDraft;
  review: ScopeReview;
  revision: number;
};

function canStart(scope: ReviewedScope | undefined): scope is ReviewedScope {
  return Boolean(
    scope?.review.scope.includes.length &&
    scope.input.includes.length &&
    scope.review.targets.length === scope.input.includes.length &&
    scope.review.targets.every((target) => target.access === "accessible"),
  );
}

function reviewFeedback(
  scope: ReviewedScope | undefined,
  problem: string | undefined,
  current: boolean,
  rejected: boolean,
) {
  const review = scope?.review;
  const ready = canStart(scope) && current && !rejected;
  return {
    review,
    ready,
    hasProblem: Boolean(
      problem || rejected || (review && (!canStart(scope) || !current)),
    ),
  };
}

function sameDraft(left: ScopeDraft, right: ScopeDraft): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function completeDraft(
  paths: (OsPath | undefined)[],
  saved?: ScopeDraft,
): ScopeDraft | undefined {
  if (paths.length === 0 || paths.some((path) => !path)) return undefined;
  return draft(paths as OsPath[], saved);
}

function hasPathEdits(paths: (OsPath | undefined)[]): boolean {
  return paths.length > 1 || paths.some((path) => path !== undefined);
}

function reviewedInputMatches(
  scope: ReviewedScope | undefined,
  current: ScopeDraft | undefined,
  revision: number,
): boolean {
  if (!scope) return true;
  return Boolean(
    current && sameDraft(current, scope.input) && scope.revision === revision,
  );
}

function reviewBlocked(
  reviewing: boolean,
  working: boolean,
  running: boolean,
  pending: boolean,
  selected: (OsPath | undefined)[],
): boolean {
  return (
    reviewing || working || running || pending || selected.some((path) => !path)
  );
}

function isCurrentReviewResponse(
  current: number,
  requested: number,
  response: number,
): boolean {
  return current === requested && response === requested;
}

function cancelScheduledReview(timer: { current: number | undefined }) {
  if (timer.current === undefined) return;
  window.clearTimeout(timer.current);
  timer.current = undefined;
}

function suppressAutomaticRetry(
  automatic: boolean,
  failedDraft: { current: string | undefined },
  submitted: ScopeDraft,
) {
  if (automatic) failedDraft.current = JSON.stringify(submitted);
}

/** Presents target selection, scope review, background collection and pinned totals. */
export function AnalysisPanel({
  client,
  preferences,
  onBrowse,
  onSizeBasisChange,
  browseTrigger,
}: {
  client: AnalysisClient;
  preferences: AnalysisDisplayPreferences;
  onBrowse?: (version: string, choice: SizeDisplay | undefined) => void;
  onSizeBasisChange?:
    ((version: string, basis: SizeBasisFilter) => void) | undefined;
  browseTrigger?: Ref<HTMLButtonElement>;
}) {
  const targets = useTargets(client);
  const reviewProblemId = useId();
  const editorId = useId();
  const scanStatusFocus = useRef<HTMLDivElement>(null);
  const editorOpener = useRef<HTMLButtonElement>(null);
  const editorBody = useRef<HTMLElement>(null);
  const firstPath = useRef<HTMLInputElement>(null);
  const reviewButton = useRef<HTMLButtonElement>(null);
  const focusReview = useRef(false);
  const focusEditor = useRef(false);
  const focusOpener = useRef(false);
  const focusResult = useRef(false);
  const autoReview = useRef<() => void>(() => undefined);
  const autoReviewTimer = useRef<number | undefined>(undefined);
  const failedAutoDraft = useRef<string | undefined>(undefined);
  const editRevision = useRef(0);
  const [displayChoice, setDisplayChoice] = useState<SizeDisplay>();
  const [editorOpen, setEditorOpen] = useState(false);
  const [selected, setSelected] = useState<(OsPath | undefined)[]>([undefined]);
  const [reviewedScope, setReviewedScope] = useState<ReviewedScope>();
  const [restoredInput, setRestoredInput] = useState<{
    input: ScopeDraft;
    revision: number;
  }>();
  const [recentOpen, setRecentOpen] = useState(false);
  const [working, setWorking] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const [pathComposing, setPathComposing] = useState(false);
  const [resultRoots, setResultRoots] = useState<OsPath[]>([]);
  const [reviewProblem, setReviewProblem] = useState<string>();
  const [rejectedRevision, setRejectedRevision] = useState<number>();
  const [error, setError] = useState<string>();
  const [receipt, setReceipt] = useState<ScanStart | null>(null);
  const [checkingDrop, setCheckingDrop] = useState(false);
  const [dropMessage, setDropMessage] = useState<{
    text: string;
    tone: "info" | "error";
  }>();
  const request = useRef(0);
  const scanBlocksDrop = useRef(false);
  const hasReceipt = useRef(false);
  const pendingDrop = useRef<{ seq: number; request: number } | undefined>(
    undefined,
  );
  const scan = useScanResult(client, receipt);
  const running = scan.running;
  const restoredIsCurrent = restoredInput?.revision === editRevision.current;
  const dropEnabled = client.subscribeFolderDrop !== undefined;
  const pathDescription =
    dropMessage?.tone === "info"
      ? dropMessage.text
      : dropEnabled
        ? "Drop a folder here or enter its path."
        : "Review this path before scanning.";

  useLayoutEffect(() => {
    if (focusEditor.current && editorOpen) {
      editorBody.current?.focus();
      focusEditor.current = false;
    }
    if (focusOpener.current && !editorOpen) {
      editorOpener.current?.focus();
      focusOpener.current = false;
    }
    if (shouldFocusReview(focusReview.current, working)) {
      reviewButton.current?.focus();
      focusReview.current = false;
    }
    if (focusResult.current && !editorOpen) {
      scanStatusFocus.current?.focus();
      focusResult.current = false;
    }
  });

  useLayoutEffect(
    () => () => {
      scanBlocksDrop.current = false;
    },
    [],
  );

  useLayoutEffect(() => {
    scanBlocksDrop.current = working || running || recentOpen;
    hasReceipt.current = receipt !== null;
  }, [receipt, running, working, recentOpen]);

  const editPaths = useCallback(
    (update: (current: (OsPath | undefined)[]) => (OsPath | undefined)[]) => {
      request.current += 1;
      editRevision.current += 1;
      failedAutoDraft.current = undefined;
      pendingDrop.current = undefined;
      setCheckingDrop(false);
      setSelected(update);
      setReviewing(false);
      setReviewProblem(undefined);
      setRejectedRevision(undefined);
      setError(undefined);
      setDropMessage(undefined);
    },
    [],
  );

  const choose = useCallback(
    (path: OsPath | undefined, index = 0) => {
      editPaths((current) =>
        current.map((entry, i) => (i === index ? path : entry)),
      );
    },
    [editPaths],
  );

  const addPath = useCallback(
    () => editPaths((current) => [...current, undefined]),
    [editPaths],
  );
  const removePath = useCallback(
    (index: number) => {
      setPathComposing(false);
      editPaths((current) => current.filter((_, i) => i !== index));
    },
    [editPaths],
  );

  function cancelSelection() {
    if (working || running) return;
    setPathComposing(false);
    editPaths(() => [undefined]);
    setReviewedScope(undefined);
    setRestoredInput(undefined);
    targets.cancel();
  }

  function changeRecentView(open: boolean) {
    setRecentOpen(open);
    if (!open) return;
    cancelScheduledReview(autoReviewTimer);
    request.current += 1;
    pendingDrop.current = undefined;
    setCheckingDrop(false);
    setReviewing(false);
  }

  function applyRecent(saved: ScopeDraft, review: ScopeReview) {
    if (request.current !== review.requestSeq || scan.running || working)
      return;
    const input = draft(saved.includes, saved);
    editPaths(() => input.includes);
    const revision = editRevision.current;
    setRestoredInput({ input, revision });
    setReviewedScope({ input, review, revision });
  }

  function chooseRecentPath() {
    if (!restoredIsCurrent) return;
    setReviewedScope(undefined);
    setReviewProblem(
      "Choose a path or reopen the saved range before scanning.",
    );
  }

  useEffect(
    () =>
      client.subscribeFolderDrop?.((drop) => {
        if (scanBlocksDrop.current) return;
        if ("pending" in drop) {
          pendingDrop.current = { seq: drop.seq, request: request.current };
          setCheckingDrop(true);
          setDropMessage(undefined);
          return;
        }
        const pending = pendingDrop.current;
        if (pending?.seq !== drop.seq) return;
        pendingDrop.current = undefined;
        setCheckingDrop(false);
        if (pending.request !== request.current) return;
        if (hasReceipt.current) {
          focusEditor.current = true;
          setEditorOpen(true);
        }
        if ("path" in drop) {
          setPathComposing(false);
          setReviewedScope(undefined);
          choose(drop.path);
          setDropMessage({
            text: "Folder selected. Review scope.",
            tone: "info",
          });
        } else {
          setDropMessage({
            text:
              drop.reason === "invalid_target"
                ? "Drop one accessible local folder. No path was changed."
                : "Folder drop is unavailable. Enter its path instead. No path was changed.",
            tone: "error",
          });
        }
      }),
    [client, choose],
  );

  async function reviewScope(automatic = false) {
    if (
      recentOpen ||
      restoredIsCurrent ||
      reviewBlocked(
        reviewing,
        working,
        running,
        Boolean(pendingDrop.current) || pathComposing,
        selected,
      )
    )
      return;
    const paths = selected.filter((path): path is OsPath => path !== undefined);
    if (paths.length === 0) return;
    const submittedDraft = draft(paths, restoredInput?.input);
    const submittedRevision = editRevision.current;
    const requestSeq = ++request.current;
    if (!automatic) cancelScheduledReview(autoReviewTimer);
    setReviewing(true);
    setReviewProblem(undefined);
    try {
      const response = await client.call("scope.review", {
        draft: submittedDraft,
        requestSeq,
      });
      if (
        isCurrentReviewResponse(
          request.current,
          requestSeq,
          response.requestSeq,
        )
      ) {
        const candidate = {
          input: submittedDraft,
          review: response,
          revision: submittedRevision,
        };
        if (canStart(candidate)) {
          setReviewedScope(candidate);
          setRejectedRevision(undefined);
          failedAutoDraft.current = undefined;
          setReviewProblem(undefined);
        } else {
          setRejectedRevision(submittedRevision);
          setReviewProblem(rejectedReviewText(response));
          suppressAutomaticRetry(automatic, failedAutoDraft, submittedDraft);
        }
      } else if (request.current === requestSeq) {
        setRejectedRevision(submittedRevision);
        setReviewProblem(
          "The scope review response did not match this request. Review again.",
        );
        suppressAutomaticRetry(automatic, failedAutoDraft, submittedDraft);
      }
    } catch (failure) {
      if (request.current === requestSeq) {
        const message = reviewedScope
          ? "Review failed. The last successful review remains available; the current paths were not accepted."
          : reviewErrorText(failure);
        setReviewProblem(message);
        if (failure instanceof AnalysisError)
          setRejectedRevision(submittedRevision);
        suppressAutomaticRetry(automatic, failedAutoDraft, submittedDraft);
      }
    } finally {
      if (request.current === requestSeq) setReviewing(false);
    }
  }

  autoReview.current = () => void reviewScope(true);

  const showEditor = receipt === null || editorOpen;
  useEffect(() => {
    // A closed editor retains the draft and suspends its scheduled review.
    if (
      !showEditor ||
      recentOpen ||
      restoredIsCurrent ||
      pathComposing ||
      selected.length === 0 ||
      selected.some((path) => !path)
    )
      return;
    const current = draft(selected as OsPath[], restoredInput?.input);
    if (
      reviewedScope &&
      reviewedScope.revision === editRevision.current &&
      sameDraft(current, reviewedScope.input)
    )
      return;
    if (failedAutoDraft.current === JSON.stringify(current)) return;
    autoReviewTimer.current = window.setTimeout(() => {
      autoReviewTimer.current = undefined;
      autoReview.current();
    }, 350);
    return () => {
      if (autoReviewTimer.current !== undefined) {
        window.clearTimeout(autoReviewTimer.current);
        autoReviewTimer.current = undefined;
      }
    };
  }, [
    pathComposing,
    recentOpen,
    restoredInput,
    restoredIsCurrent,
    reviewedScope,
    selected,
    showEditor,
  ]);

  async function start() {
    if (
      scanBlocksDrop.current ||
      pendingDrop.current ||
      reviewing ||
      pathComposing
    )
      return;
    if (
      !canStart(reviewedScope) ||
      !reviewedInputIsCurrent ||
      rejectedRevision === editRevision.current
    )
      return;
    const { input, review } = reviewedScope;
    scanBlocksDrop.current = true;
    setWorking(true);
    setError(undefined);
    try {
      const result = await client.call("scan.start", {
        draft: input,
        reviewFingerprint: review.reviewFingerprint,
      });
      if (result.jobRef !== receipt?.jobRef) setDisplayChoice(undefined);
      setResultRoots(review.scope.includes);
      focusResult.current = true;
      setEditorOpen(false);
      setReceipt(result);
    } catch (failure) {
      scanBlocksDrop.current = false;
      if (restoredIsCurrent) editorBody.current?.focus();
      else focusReview.current = true;
      if (
        failure instanceof AnalysisError &&
        failure.code === "STALE_VERSION"
      ) {
        setReviewedScope(undefined);
        if (restoredIsCurrent) {
          setReviewProblem(
            "Reopen the saved range or select its paths again before scanning.",
          );
        }
      }
      setError(errorText(failure));
    } finally {
      setWorking(false);
    }
  }

  const resultActive = receipt !== null || error !== undefined;
  const editorBusy = isEditorBusy(working, reviewing, checkingDrop);
  const currentInput = completeDraft(selected, restoredInput?.input);
  const reviewedInputIsCurrent = reviewedInputMatches(
    reviewedScope,
    currentInput,
    editRevision.current,
  );
  const { review, ready, hasProblem } = reviewFeedback(
    reviewedScope,
    reviewProblem,
    reviewedInputIsCurrent,
    rejectedRevision === editRevision.current,
  );

  function renderEditorBody() {
    return (
      <>
        <section
          ref={editorBody}
          className="du-target-editor__body"
          aria-label="Target selection and scope review"
          tabIndex={0}
        >
          {client.recentScopesEnabled && (
            <RecentScopePicker
              client={client}
              hasDraftEdits={hasPathEdits(selected)}
              nextRequestSeq={() => ++request.current}
              isRequestSeqCurrent={(seq) => request.current === seq}
              onApply={applyRecent}
              onChoosePath={chooseRecentPath}
              onOpenChange={changeRecentView}
              isDisabled={working || running || pathComposing}
              pathFocusRef={firstPath}
              applyFocusRef={editorBody}
              scrollContainerRef={editorBody}
            />
          )}
          <div hidden={recentOpen}>
            <TargetPicker
              targets={targets}
              isInitial={!resultActive}
              disabled={working || running}
              selected={selected}
              choose={choose}
              addPath={addPath}
              removePath={removePath}
              dropEnabled={dropEnabled}
              dropError={
                dropMessage?.tone === "error" ? dropMessage.text : undefined
              }
              pathDescription={pathDescription}
              reviewDescriptionId={hasProblem ? reviewProblemId : undefined}
              feedback={
                <ScopeReviewFeedback
                  review={review}
                  ready={ready}
                  current={reviewedInputIsCurrent}
                  latestAccepted={
                    reviewedInputIsCurrent &&
                    rejectedRevision !== editRevision.current
                  }
                  problem={reviewProblem}
                  problemId={reviewProblemId}
                />
              }
              onPathCompositionChange={setPathComposing}
              primaryPathRef={firstPath}
            />
          </div>
        </section>
        {!recentOpen && (
          <ScopeActions
            reviewButton={reviewButton}
            checkingDrop={checkingDrop}
            composing={pathComposing}
            reviewing={reviewing}
            working={working}
            running={running}
            reviewEnabled={!restoredIsCurrent && selected.every(Boolean)}
            startEnabled={ready}
            onReview={() => void reviewScope()}
            onStart={() => void start()}
            onCancel={cancelSelection}
          />
        )}
      </>
    );
  }

  return (
    <section
      className="du-analysis du-analysis--workspace"
      aria-labelledby="analysis-heading"
    >
      <AnalysisHeader
        hasTotal={Boolean(scan.total)}
        resultActive={resultActive}
        hasReceipt={receipt !== null}
        running={running}
        busy={editorBusy}
        editorOpen={editorOpen}
        editorId={editorId}
        opener={editorOpener}
        onOpen={() => {
          focusEditor.current = true;
          setEditorOpen(true);
        }}
      />
      <div
        className="du-analysis__grid"
        data-result-active={resultActive || undefined}
        data-editor-open={showEditor || undefined}
      >
        {showEditor && (
          <section
            id={editorId}
            className="du-target-editor"
            aria-label="Target and scope"
          >
            {resultActive && (
              <header className="du-target-editor__header">
                <Heading level={3}>Scan targets</Heading>
                {receipt !== null && (
                  <Action
                    variant="quiet"
                    isDisabled={editorBusy}
                    onPress={() => {
                      cancelScheduledReview(autoReviewTimer);
                      setPathComposing(false);
                      changeRecentView(false);
                      focusOpener.current = true;
                      setEditorOpen(false);
                    }}
                  >
                    Close target editor
                  </Action>
                )}
              </header>
            )}
            {renderEditorBody()}
          </section>
        )}
        {resultActive && (
          <section
            className="du-analysis__result"
            aria-label="Scan and results"
          >
            <ScanOutput
              client={client}
              preferences={preferences}
              displayChoice={displayChoice}
              onDisplayChoice={setDisplayChoice}
              onSizeBasisChange={onSizeBasisChange}
              scan={scan}
              started={receipt !== null}
              running={running}
              problem={error}
              statusFocus={scanStatusFocus}
              roots={resultRoots}
              actions={
                <ResultActions
                  version={scan.total?.resultVersion}
                  onBrowse={bindBrowse(onBrowse, displayChoice)}
                  trigger={browseTrigger}
                  running={running}
                  state={scan.status?.state}
                  pending={scan.control}
                  onControl={scan.requestControl}
                  statusFocus={scanStatusFocus}
                />
              }
            />
          </section>
        )}
      </div>
    </section>
  );
}

function AnalysisHeader({
  hasTotal,
  resultActive,
  hasReceipt,
  running,
  busy,
  editorOpen,
  editorId,
  opener,
  onOpen,
}: {
  hasTotal: boolean;
  resultActive: boolean;
  hasReceipt: boolean;
  running: boolean;
  busy: boolean;
  editorOpen: boolean;
  editorId: string;
  opener: Ref<HTMLButtonElement>;
  onOpen: () => void;
}) {
  return (
    <header className="du-analysis__header">
      <Heading level={2} id="analysis-heading">
        {hasTotal
          ? "Disk space results"
          : resultActive
            ? "Scan a drive or folder"
            : "Choose what to analyze"}
      </Heading>
      {hasReceipt && !running && (
        <Action
          ref={opener}
          isDisabled={busy}
          aria-expanded={editorOpen}
          {...(editorOpen ? { "aria-controls": editorId } : {})}
          onPress={onOpen}
        >
          Analyze again
        </Action>
      )}
      {!resultActive && (
        <Text kind="muted">
          Enter a folder path or choose a drive, then review the scope before
          scanning.
        </Text>
      )}
    </header>
  );
}

function isEditorBusy(
  working: boolean,
  reviewing: boolean,
  checkingDrop: boolean,
) {
  return working || reviewing || checkingDrop;
}

function shouldFocusReview(pending: boolean, working: boolean) {
  return pending && !working;
}

function bindBrowse(
  open:
    ((version: string, choice: SizeDisplay | undefined) => void) | undefined,
  choice: SizeDisplay | undefined,
): ((version: string) => void) | undefined {
  return open ? (version) => open(version, choice) : undefined;
}
