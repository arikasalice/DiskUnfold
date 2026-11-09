import { useRef, useState } from "react";
import { AnalysisError, windowsPath } from "../../ipc";
import type {
  AnalysisClient,
  OsPath,
  ScanStart,
  ScopeDraft,
  ScopeReview,
  SpaceTotal,
  SpaceTotalSize,
} from "../../ipc";
import { isRunning, useScanResult } from "../../state/analysis-scan";
import { useTargets } from "../../state/analysis-targets";

function errorText(error: unknown): string {
  if (error instanceof AnalysisError && error.code === "STALE_VERSION") {
    return "The target or result changed. Review the scope again.";
  }
  return error instanceof Error ? error.message : "The operation failed";
}

const sizes = {
  logical: "Logical size",
  allocated: "Allocated size",
  unique_physical: "Unique physical size",
};

function displaySize({ size }: SpaceTotalSize): string {
  if (size.state === "unknown") return "Unavailable";
  const bytes = BigInt(size.bytes).toLocaleString();
  return `${size.state === "lower_bound" ? "At least " : ""}${bytes} bytes`;
}

/** Shows the distinct kinds of known gaps in this pinned result. */
function Coverage({ total }: { total: SpaceTotal }) {
  const counters = [
    { label: "Missing items", count: total.missingItems },
    { label: "Inaccessible ranges", count: total.inaccessibleRanges },
    { label: "Untraversed links", count: total.untraversedLinks },
  ];
  const present = counters.filter(({ count }) => count !== "0");
  return (
    <ul aria-label="Coverage limits">
      {present.map(({ label, count }) => (
        <li key={label}>
          {label}:{" "}
          {count === null ? "Not recorded" : BigInt(count).toLocaleString()}
        </li>
      ))}
    </ul>
  );
}

function draft(path: OsPath): ScopeDraft {
  return {
    includes: [path],
    exclusions: [],
    boundary: "current_volume",
    includedMounts: [],
    source: "gui",
  };
}

function TargetPicker({
  targets,
  disabled,
  selected,
  choose,
}: {
  targets: ReturnType<typeof useTargets>;
  disabled: boolean;
  selected: OsPath | undefined;
  choose: (path: OsPath | undefined) => void;
}) {
  const error = targets.error ?? targets.list?.unavailableReason;
  const notice =
    error === "os.unsupported"
      ? "Automatic target listing is unavailable. Enter a local folder path."
      : error;
  return (
    <>
      <button
        type="button"
        disabled={targets.loading || disabled}
        onClick={() => void targets.refresh(true)}
      >
        Refresh targets
      </button>
      {targets.loading && <p role="status">Loading targets…</p>}
      {notice && <p role="alert">{notice}</p>}
      <fieldset disabled={disabled}>
        <legend>Choose a target</legend>
        {targets.list?.targets.map((target) => {
          const probe = targets.probes[target.path.raw];
          const capacity = probe?.totalBytes;
          const access =
            probe?.access ??
            (targets.finished && target.access === "checking"
              ? "unknown"
              : target.access);
          return (
            <div key={target.path.raw}>
              <label>
                <input
                  type="radio"
                  name="analysis-target"
                  checked={selected?.raw === target.path.raw}
                  onChange={() => choose(target.path)}
                />
                {target.path.display}
              </label>{" "}
              <span>{access}</span>
              {capacity && (
                <span> · {BigInt(capacity).toLocaleString()} bytes total</span>
              )}
            </div>
          );
        })}
        <label>
          Windows folder path
          <input
            type="text"
            value={selected?.display ?? ""}
            onChange={(event) =>
              choose(
                event.target.value
                  ? windowsPath(event.target.value)
                  : undefined,
              )
            }
          />
        </label>
      </fieldset>
    </>
  );
}

function ScanOutput({
  scan,
  started,
  problem,
}: {
  scan: ReturnType<typeof useScanResult>;
  started: boolean;
  problem: string | undefined;
}) {
  const error = problem ?? scan.error ?? scan.status?.reasonKey;
  const state = scan.unavailable
    ? "unavailable"
    : (scan.status?.state ?? "queued");
  return (
    <>
      {started && <p role="status">Scan status: {state}</p>}
      {error && <p role="alert">{error}</p>}
      {scan.total && (
        <section aria-label="Space totals">
          <h3>Space totals</h3>
          <p>Result completeness: {scan.total.completeness}</p>
          <Coverage total={scan.total} />
          <dl>
            {scan.total.sizes.map((value) => (
              <div key={value.size.basis}>
                <dt>{sizes[value.size.basis]}</dt>
                <dd>{displaySize(value)}</dd>
              </div>
            ))}
          </dl>
        </section>
      )}
    </>
  );
}

/** Presents target selection, scope review, background collection and pinned totals. */
export function AnalysisPanel({ client }: { client: AnalysisClient }) {
  const targets = useTargets(client);
  const [selected, setSelected] = useState<OsPath>();
  const [review, setReview] = useState<ScopeReview>();
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string>();
  const [receipt, setReceipt] = useState<ScanStart | null>(null);
  const [cancelRequested, setCancelRequested] = useState(false);
  const request = useRef(0);
  const scan = useScanResult(client, receipt);
  const running =
    receipt !== null &&
    !scan.unavailable &&
    (!scan.status || isRunning(scan.status.state));

  function choose(path: OsPath | undefined) {
    request.current += 1;
    setSelected(path);
    setReview(undefined);
    setWorking(false);
    setError(undefined);
  }

  async function reviewScope() {
    if (!selected) return;
    const requestSeq = ++request.current;
    setWorking(true);
    setError(undefined);
    try {
      const response = await client.call("scope.review", {
        draft: draft(selected),
        requestSeq,
      });
      if (request.current === response.requestSeq) setReview(response);
    } catch (failure) {
      if (request.current === requestSeq) setError(errorText(failure));
    } finally {
      if (request.current === requestSeq) setWorking(false);
    }
  }

  async function start() {
    if (!review || !selected) return;
    setWorking(true);
    setCancelRequested(false);
    setError(undefined);
    try {
      setReceipt(
        await client.call("scan.start", {
          draft: draft(selected),
          reviewFingerprint: review.reviewFingerprint,
        }),
      );
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
    setCancelRequested(true);
    try {
      const response = await client.call("job.cancel", {
        jobRef: receipt.jobRef,
      });
      if (response.outcome === "unsupported") {
        setError(response.reasonKey ?? "Cancellation is unavailable");
      }
    } catch (failure) {
      setCancelRequested(false);
      setError(errorText(failure));
    }
  }

  return (
    <section aria-labelledby="analysis-heading">
      <h2 id="analysis-heading">Disk analysis</h2>
      <TargetPicker
        targets={targets}
        disabled={working || running}
        selected={selected}
        choose={choose}
      />
      <button
        type="button"
        disabled={!selected || working || running}
        onClick={() => void reviewScope()}
      >
        Review scope
      </button>{" "}
      <button
        type="button"
        disabled={
          !review ||
          review.targets.some((target) => target.access !== "accessible") ||
          working ||
          running
        }
        onClick={() => void start()}
      >
        Start scan
      </button>{" "}
      {running && (
        <button
          type="button"
          disabled={cancelRequested}
          onClick={() => void cancel()}
        >
          Cancel scan
        </button>
      )}
      {review && (
        <p>
          Reviewed scope:{" "}
          {review.scope.includes.map((path) => path.display).join(", ")}
        </p>
      )}
      {review?.notices.map((notice) => (
        <p key={notice}>{notice}</p>
      ))}
      <ScanOutput scan={scan} started={receipt !== null} problem={error} />
    </section>
  );
}
