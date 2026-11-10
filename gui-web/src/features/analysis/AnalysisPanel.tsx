import { useCallback, useEffect, useRef, useState } from "react";
import { Action, Heading, Notice, Readout, Text } from "../../components";
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

function draft(path: OsPath): ScopeDraft {
  return {
    includes: [path],
    exclusions: [],
    boundary: "current_volume",
    includedMounts: [],
    source: "gui",
  };
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
  const [dropMessage, setDropMessage] = useState<{
    text: string;
    tone: "info" | "error";
  }>();
  const request = useRef(0);
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

  const choose = useCallback((path: OsPath | undefined) => {
    request.current += 1;
    setSelected(path);
    setReview(undefined);
    setWorking(false);
    setError(undefined);
    setDropMessage(undefined);
  }, []);

  useEffect(
    () =>
      client.subscribeFolderDrop?.((drop) => {
        if (working || running) return;
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
    <section className="du-analysis" aria-labelledby="analysis-heading">
      <header className="du-analysis__header">
        <Text kind="eyebrow">Analysis workspace</Text>
        <Heading level={2} id="analysis-heading">
          Disk analysis
        </Heading>
        <Text kind="muted">
          Review a scope before scanning. See how much space the selected scope
          uses.
        </Text>
      </header>
      <div className="du-analysis__grid">
        <section className="du-card" aria-label="Target and scope">
          <TargetPicker
            targets={targets}
            disabled={working || running}
            selected={selected}
            choose={choose}
            dropEnabled={dropEnabled}
            dropError={
              dropMessage?.tone === "error" ? dropMessage.text : undefined
            }
            pathDescription={pathDescription}
          />
          <footer className="du-card__actions">
            <Action
              icon="folder"
              isDisabled={!selected || working || running}
              onPress={() => void reviewScope()}
            >
              Review scope
            </Action>
            <Action
              icon="scan"
              variant="primary"
              isDisabled={
                !review ||
                review.targets.some(
                  (target) => target.access !== "accessible",
                ) ||
                working ||
                running
              }
              onPress={() => void start()}
            >
              Start scan
            </Action>
          </footer>
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
          {review?.notices.map((notice) => (
            <Notice key={notice}>{notice}</Notice>
          ))}
        </section>
        <section className="du-card" aria-label="Scan and results">
          <ScanOutput
            scan={scan}
            started={receipt !== null}
            running={running}
            problem={error}
          />
          {running && (
            <footer className="du-card__actions">
              <Action
                isDisabled={cancelRequested}
                onPress={() => void cancel()}
              >
                Cancel scan
              </Action>
            </footer>
          )}
        </section>
      </div>
    </section>
  );
}
