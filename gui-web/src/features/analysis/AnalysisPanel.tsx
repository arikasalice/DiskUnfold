import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { Ref } from "react";
import {
  Action,
  Heading,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
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
import { ScanOutput, TargetPicker } from "./AnalysisViews";

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
    default:
      return key;
  }
}

function ReviewNotices({
  review,
  input,
  problem,
}: {
  review: ScopeReview | undefined;
  input: ScopeDraft | undefined;
  problem: string | undefined;
}) {
  if (problem) return <Notice tone="error">{problem}</Notice>;
  if (!review) return null;
  return (
    <>
      {review.notices.map((notice) => (
        <Notice key={notice}>{noticeText(notice)}</Notice>
      ))}
      {!canStart(review, input) && (
        <Notice tone="error">
          One or more selected paths could not be verified. Review all paths
          before starting.
        </Notice>
      )}
    </>
  );
}

function ScopeActions({
  reviewing,
  working,
  running,
  reviewEnabled,
  startEnabled,
  onReview,
  onStart,
}: {
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
    <>
      {reviewing && <Progress label="Reviewing scope" />}
      {starting && <Progress label="Starting scan" />}
      <footer className="du-card__actions">
        <Action
          icon="folder"
          isDisabled={blocked || !reviewEnabled}
          onPress={onReview}
        >
          {reviewing ? "Reviewing scope…" : "Review scope"}
        </Action>
        <Action
          icon="scan"
          variant="primary"
          isDisabled={blocked || !startEnabled}
          onPress={onStart}
        >
          {starting ? "Starting scan…" : "Start scan"}
        </Action>
      </footer>
    </>
  );
}

function ResultActions({
  version,
  onBrowse,
  trigger,
  running,
  cancelRequested,
  onCancel,
}: {
  version: string | undefined;
  onBrowse: ((version: string) => void) | undefined;
  trigger: Ref<HTMLButtonElement> | undefined;
  running: boolean;
  cancelRequested: boolean;
  onCancel: () => void;
}) {
  const browse = version && onBrowse && (
    <Action
      ref={trigger ?? null}
      icon="folder"
      onPress={() => onBrowse(version)}
    >
      Browse folders
    </Action>
  );
  if (!browse && !running) return null;
  return (
    <div className="du-card__actions">
      {browse}
      {running && (
        <Action isDisabled={cancelRequested} onPress={onCancel}>
          Cancel scan
        </Action>
      )}
    </div>
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
  const [dropMessage, setDropMessage] = useState<{
    text: string;
    tone: "info" | "error";
  }>();
  const request = useRef(0);
  const cancelOwner = useRef<{ jobRef: string }>(undefined);
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
    },
    [],
  );

  const editPaths = useCallback(
    (update: (current: (OsPath | undefined)[]) => (OsPath | undefined)[]) => {
      request.current += 1;
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
        if (working || running) return;
        if ("pending" in drop) {
          pendingDrop.current = { seq: drop.seq, request: request.current };
          return;
        }
        const pending = pendingDrop.current;
        if (pending?.seq !== drop.seq) return;
        pendingDrop.current = undefined;
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
                ? "Drop one accessible local folder."
                : "Folder drop is unavailable. Enter its path instead.",
            tone: "error",
          });
        }
      }),
    [client, choose, running, working],
  );

  async function reviewScope() {
    if (selected.some((path) => !path)) return;
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
    if (!review || !reviewedDraft || !canStart(review, reviewedDraft)) return;
    cancelOwner.current = undefined;
    pendingDrop.current = undefined;
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

  return (
    <section className="du-analysis" aria-labelledby="analysis-heading">
      <header className="du-analysis__header">
        <Heading level={2} id="analysis-heading">
          Disk analysis
        </Heading>
        <Text kind="muted">
          Review a scope, then scan it to see its space totals.
        </Text>
      </header>
      <div className="du-analysis__grid">
        <section className="du-card" aria-label="Target and scope">
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
          />
          {review && (
            <Readout
              label="Reviewed scope"
              rows={[
                {
                  label: "Reviewed scope",
                  value: review.scope.includes
                    .map((path) => path.display)
                    .join(", "),
                },
                { label: "Boundary", value: "Current volume" },
              ]}
            />
          )}
          <ReviewNotices
            review={review}
            input={reviewedDraft}
            problem={reviewProblem}
          />
          <ScopeActions
            reviewing={reviewing}
            working={working}
            running={running}
            reviewEnabled={selected.every(Boolean)}
            startEnabled={canStart(review, reviewedDraft)}
            onReview={() => void reviewScope()}
            onStart={() => void start()}
          />
        </section>
        <section className="du-card" aria-label="Scan and results">
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
        </section>
      </div>
    </section>
  );
}
