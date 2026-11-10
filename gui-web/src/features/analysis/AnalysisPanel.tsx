import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { Ref } from "react";
import { Heading, Text } from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  OsPath,
  ScanStart,
  ScopeDraft,
  ScopeReview,
} from "../../ipc";
import { isRunning, useScanResult } from "../../state/analysis-scan";
import { useTargets } from "../../state/analysis-targets";
import {
  ResultActions,
  ScopeActions,
  ScopeReviewFeedback,
} from "./AnalysisActions";
import { ScanOutput } from "./AnalysisViews";
import { TargetPicker } from "./TargetPicker";

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
  onBrowse,
  browseTrigger,
}: {
  client: AnalysisClient;
  onBrowse?: (version: string) => void;
  browseTrigger?: Ref<HTMLButtonElement>;
}) {
  const targets = useTargets(client);
  const reviewProblemId = useId();
  const [selected, setSelected] = useState<(OsPath | undefined)[]>([undefined]);
  const [review, setReview] = useState<ScopeReview>();
  const [reviewedDraft, setReviewedDraft] = useState<ScopeDraft>();
  const [working, setWorking] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const [resultRoots, setResultRoots] = useState<OsPath[]>([]);
  const [reviewProblem, setReviewProblem] = useState<string>();
  const [error, setError] = useState<string>();
  const [receipt, setReceipt] = useState<ScanStart | null>(null);
  const [cancelRequested, setCancelRequested] = useState(false);
  const [checkingDrop, setCheckingDrop] = useState(false);
  const [dropMessage, setDropMessage] = useState<{
    text: string;
    tone: "info" | "error";
  }>();
  const request = useRef(0);
  const cancelOwner = useRef<{ jobRef: string }>(undefined);
  const scanBlocksDrop = useRef(false);
  const pendingDrop = useRef<{ seq: number; request: number } | undefined>(
    undefined,
  );
  const scan = useScanResult(client, receipt);
  const running =
    receipt !== null &&
    !scan.unavailable &&
    (!scan.status || isRunning(scan.status.state));
  const dropEnabled = client.subscribeFolderDrop !== undefined;
  const pathDescription =
    dropMessage?.tone === "info"
      ? dropMessage.text
      : dropEnabled
        ? "Drop a folder here or enter its path."
        : "Review this path before scanning.";

  useLayoutEffect(
    () => () => {
      cancelOwner.current = undefined;
      scanBlocksDrop.current = false;
    },
    [],
  );

  useLayoutEffect(() => {
    scanBlocksDrop.current = working || running;
  }, [running, working]);

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
    if (pendingDrop.current || selected.some((path) => !path)) return;
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
    cancelOwner.current = undefined;
    setWorking(true);
    setCancelRequested(false);
    setError(undefined);
    try {
      const result = await client.call("scan.start", {
        draft: reviewedDraft,
        reviewFingerprint: review.reviewFingerprint,
      });
      setResultRoots(review.scope.includes);
      setReceipt(result);
    } catch (failure) {
      scanBlocksDrop.current = false;
      if (failure instanceof AnalysisError && failure.code === "STALE_VERSION")
        setReview(undefined);
      setError(errorText(failure));
    } finally {
      setWorking(false);
    }
  }

  async function cancel() {
    if (!receipt) return;
    const owner = { jobRef: receipt.jobRef };
    cancelOwner.current = owner;
    setCancelRequested(true);
    try {
      const response = await client.call("job.cancel", {
        jobRef: owner.jobRef,
      });
      if (cancelOwner.current !== owner) return;
      if (response.outcome === "unsupported") {
        setCancelRequested(false);
        setError("Cancellation is unavailable for this scan.");
      }
    } catch (failure) {
      if (cancelOwner.current !== owner) return;
      setCancelRequested(false);
      setError(errorText(failure));
    }
  }

  const ready = canStart(review, reviewedDraft);
  const resultActive = receipt !== null || error !== undefined;
  const scopeProblem = Boolean(reviewProblem || (review && !ready));

  return (
    <section
      className="du-analysis du-analysis--workspace"
      aria-labelledby="analysis-heading"
    >
      <header className="du-analysis__header">
        <Heading level={2} id="analysis-heading">
          {scan.total ? "Disk space results" : "Scan a drive or folder"}
        </Heading>
        {!resultActive && (
          <Text kind="muted">
            Choose a drive or enter a folder path. Review the scope before
            scanning.
          </Text>
        )}
      </header>
      <div
        className="du-analysis__grid"
        data-result-active={resultActive || undefined}
      >
        <section className="du-target-editor" aria-label="Target and scope">
          {resultActive && (
            <header className="du-target-editor__header">
              <Heading level={3}>Scan targets</Heading>
            </header>
          )}
          <section
            className="du-target-editor__body"
            aria-label="Target selection and scope review"
            tabIndex={0}
          >
            <TargetPicker
              targets={targets}
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
            checkingDrop={checkingDrop}
            reviewing={reviewing}
            working={working}
            running={running}
            reviewEnabled={selected.every(Boolean)}
            startEnabled={ready}
            onReview={() => void reviewScope()}
            onStart={() => void start()}
          />
        </section>
        <section className="du-analysis__result" aria-label="Scan and results">
          {resultActive ? (
            <ScanOutput
              client={client}
              scan={scan}
              started={receipt !== null}
              running={running}
              cancelRequested={cancelRequested}
              problem={error}
              roots={resultRoots}
              actions={
                <ResultActions
                  version={scan.total?.resultVersion}
                  onBrowse={onBrowse}
                  trigger={browseTrigger}
                  running={running}
                  cancelRequested={cancelRequested}
                  onCancel={() => void cancel()}
                />
              }
            />
          ) : (
            <div className="du-analysis__guide">
              <Heading level={3}>After the scan</Heading>
              <Text kind="muted">
                See measured space and coverage, then browse the scanned
                folders.
              </Text>
            </div>
          )}
        </section>
      </div>
    </section>
  );
}
