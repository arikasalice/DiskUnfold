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
} from "./AnalysisActions";
import { ScanOutput } from "./AnalysisViews";
import { TargetPicker } from "./TargetPicker";
import type { SizeDisplay } from "../../i18n/bytes";
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

function draft(paths: OsPath[]): ScopeDraft {
  return {
    includes: paths,
    exclusions: [],
    boundary: "current_volume",
    includedMounts: [],
    source: "gui",
  };
}

function canStart(
  review: ScopeReview | undefined,
  input: ScopeDraft | undefined,
): boolean {
  return Boolean(
    review?.scope.includes.length &&
    input?.includes.length &&
    review.targets.length === input.includes.length &&
    review.targets.every((target) => target.access === "accessible"),
  );
}

/** Presents target selection, scope review, background collection and pinned totals. */
export function AnalysisPanel({
  client,
  preferences,
  onBrowse,
  browseTrigger,
}: {
  client: AnalysisClient;
  preferences: AnalysisDisplayPreferences;
  onBrowse?: (version: string, choice: SizeDisplay | undefined) => void;
  browseTrigger?: Ref<HTMLButtonElement>;
}) {
  const targets = useTargets(client);
  const reviewProblemId = useId();
  const editorId = useId();
  const scanStatusFocus = useRef<HTMLDivElement>(null);
  const editorOpener = useRef<HTMLButtonElement>(null);
  const editorBody = useRef<HTMLElement>(null);
  const reviewButton = useRef<HTMLButtonElement>(null);
  const focusReview = useRef(false);
  const focusEditor = useRef(false);
  const focusOpener = useRef(false);
  const focusResult = useRef(false);
  const [displayChoice, setDisplayChoice] = useState<SizeDisplay>();
  const [editorOpen, setEditorOpen] = useState(false);
  const [selected, setSelected] = useState<(OsPath | undefined)[]>([undefined]);
  const [review, setReview] = useState<ScopeReview>();
  const [reviewedDraft, setReviewedDraft] = useState<ScopeDraft>();
  const [working, setWorking] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const [resultRoots, setResultRoots] = useState<OsPath[]>([]);
  const [reviewProblem, setReviewProblem] = useState<string>();
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
    scanBlocksDrop.current = working || running;
    hasReceipt.current = receipt !== null;
  }, [receipt, running, working]);

  const editPaths = useCallback(
    (update: (current: (OsPath | undefined)[]) => (OsPath | undefined)[]) => {
      request.current += 1;
      pendingDrop.current = undefined;
      setCheckingDrop(false);
      setSelected(update);
      setReview(undefined);
      setReviewedDraft(undefined);
      setReviewing(false);
      setReviewProblem(undefined);
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
    (index: number) =>
      editPaths((current) => current.filter((_, i) => i !== index)),
    [editPaths],
  );

  function cancelSelection() {
    if (working || running) return;
    editPaths(() => [undefined]);
    targets.cancel();
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

  async function reviewScope() {
    if (
      reviewing ||
      working ||
      running ||
      pendingDrop.current ||
      selected.some((path) => !path)
    )
      return;
    const paths = selected.filter((path): path is OsPath => path !== undefined);
    if (paths.length === 0) return;
    const requestSeq = ++request.current;
    setReview(undefined);
    setReviewedDraft(undefined);
    setReviewing(true);
    setReviewProblem(undefined);
    try {
      const submittedDraft = draft(paths);
      const response = await client.call("scope.review", {
        draft: submittedDraft,
        requestSeq,
      });
      if (
        request.current === requestSeq &&
        response.requestSeq === requestSeq
      ) {
        setReview(response);
        setReviewedDraft(submittedDraft);
      }
    } catch (failure) {
      if (request.current === requestSeq)
        setReviewProblem(reviewErrorText(failure));
    } finally {
      if (request.current === requestSeq) setReviewing(false);
    }
  }

  async function start() {
    if (scanBlocksDrop.current || pendingDrop.current) return;
    if (!review || !reviewedDraft || !canStart(review, reviewedDraft)) return;
    scanBlocksDrop.current = true;
    setWorking(true);
    setError(undefined);
    try {
      const result = await client.call("scan.start", {
        draft: reviewedDraft,
        reviewFingerprint: review.reviewFingerprint,
      });
      if (result.jobRef !== receipt?.jobRef) setDisplayChoice(undefined);
      setResultRoots(review.scope.includes);
      focusResult.current = true;
      setEditorOpen(false);
      setReceipt(result);
    } catch (failure) {
      scanBlocksDrop.current = false;
      focusReview.current = true;
      if (failure instanceof AnalysisError && failure.code === "STALE_VERSION")
        setReview(undefined);
      setError(errorText(failure));
    } finally {
      setWorking(false);
    }
  }

  const ready = canStart(review, reviewedDraft);
  const resultActive = receipt !== null || error !== undefined;
  const scopeProblem = Boolean(reviewProblem || (review && !ready));
  const showEditor = receipt === null || editorOpen;
  const editorBusy = isEditorBusy(working, reviewing, checkingDrop);

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
                      focusOpener.current = true;
                      setEditorOpen(false);
                    }}
                  >
                    Close target editor
                  </Action>
                )}
              </header>
            )}
            <section
              ref={editorBody}
              className="du-target-editor__body"
              aria-label="Target selection and scope review"
              tabIndex={0}
            >
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
                reviewDescriptionId={scopeProblem ? reviewProblemId : undefined}
                feedback={
                  <ScopeReviewFeedback
                    review={review}
                    ready={ready}
                    problem={reviewProblem}
                    problemId={reviewProblemId}
                  />
                }
              />
            </section>
            <ScopeActions
              reviewButton={reviewButton}
              checkingDrop={checkingDrop}
              reviewing={reviewing}
              working={working}
              running={running}
              reviewEnabled={selected.every(Boolean)}
              startEnabled={ready}
              onReview={() => void reviewScope()}
              onStart={() => void start()}
              onCancel={cancelSelection}
            />
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
