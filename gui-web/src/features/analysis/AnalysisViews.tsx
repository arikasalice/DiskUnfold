import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, Ref } from "react";
import {
  Action,
  Heading,
  LiveStatus,
  Metric,
  Notice,
  Progress,
  Readout,
  StatusBadge,
  Text,
} from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  BrowseDetail,
  BrowseValue,
  JobState,
  OsPath,
  SpaceTotal,
} from "../../ipc";
import type { ScanControl, useScanResult } from "../../state/analysis-scan";
import { ScanIssues } from "./ScanIssues";
import { ItemSpaceDetails } from "./ItemSpaceDetails";
import { SizeDisplayControls } from "./SizeDisplayControls";
import { VolumeExplanation } from "./VolumeExplanation";
import {
  displayMeasurement,
  displaySize,
  displaySizeWithBytes,
  formatBytes,
  sizeLabels,
  windowsSizeDisplay,
} from "./space-format";
import type { SizeBasisFilter, SizeDisplay } from "./space-format";

const resultVersionLabels = {
  complete: "Final",
  partial: "Interim",
  missing: "Final with scan gaps",
  cancelled: "Cancelled",
} satisfies Record<SpaceTotal["completeness"], string>;

function Coverage({ total }: { total: SpaceTotal }) {
  const counters = [
    { label: "Missing items", count: total.missingItems },
    { label: "Inaccessible ranges", count: total.inaccessibleRanges },
    { label: "Untraversed links", count: total.untraversedLinks },
  ];
  return (
    <Readout
      label="Coverage limits"
      rows={counters
        .filter(({ count }) => count !== "0")
        .map(({ label, count }) => ({
          label,
          value:
            count === null ? "Not recorded" : BigInt(count).toLocaleString(),
        }))}
    />
  );
}

/** Shows only totals from the scan's pinned result, with basis and coverage kept separate. */
export function ScanOutput({
  client,
  scan,
  started,
  running,
  problem,
  statusFocus,
  roots,
  actions,
}: {
  client: AnalysisClient;
  scan: ReturnType<typeof useScanResult>;
  started: boolean;
  running: boolean;
  problem: string | undefined;
  statusFocus: Ref<HTMLDivElement>;
  roots: readonly OsPath[];
  actions: ReactNode;
}) {
  const error =
    problem ?? scan.controlError ?? scan.error ?? scan.status?.reasonKey;
  const awaitingTotals =
    !running &&
    Boolean(scan.status?.resultVersion) &&
    !scan.total &&
    !scan.error;
  return (
    <>
      <ScanOutputHeader
        scan={scan}
        started={started}
        running={running}
        awaitingTotals={awaitingTotals}
        statusFocus={statusFocus}
        actions={actions}
      />
      <section
        className="du-result__body"
        aria-label="Result details"
        tabIndex={0}
      >
        {error && <Notice tone="error">{error}</Notice>}
        {scan.total ? (
          <PinnedResult client={client} total={scan.total} roots={roots} />
        ) : running ? (
          <Text kind="muted">
            {scan.status?.state === "paused"
              ? "The scan is paused. Resume to continue collecting file metadata."
              : "Collecting file metadata. Totals appear when a result is ready."}
          </Text>
        ) : !awaitingTotals && !error ? (
          <Text kind="muted">No result to display.</Text>
        ) : null}
      </section>
    </>
  );
}

function ScanOutputHeader({
  scan,
  started,
  running,
  awaitingTotals,
  statusFocus,
  actions,
}: {
  scan: ReturnType<typeof useScanResult>;
  started: boolean;
  running: boolean;
  awaitingTotals: boolean;
  statusFocus: Ref<HTMLDivElement>;
  actions: ReactNode;
}) {
  const cancelling =
    scan.control === "cancel" || scan.status?.state === "cancel_requested";
  return (
    <header className="du-result__header">
      <div className="du-result__title">
        <Heading level={3}>
          {scan.total ? "Space totals" : "Scan progress"}
        </Heading>
        {started && (
          <ScanStatus
            state={scan.unavailable ? "unavailable" : scan.status?.state}
            pending={running ? scan.control : undefined}
            focusTarget={statusFocus}
          />
        )}
      </div>
      <div className="du-result__toolbar">
        <ResultVersionSummary
          total={scan.total}
          hasNewResult={scan.hasNewResult}
          showLatest={scan.showLatest}
          switching={scan.switchingResult}
          finished={scan.status?.state === "done"}
        />
        {actions}
      </div>
      {running && (cancelling || scan.status?.state !== "paused") && (
        <Progress
          label={cancelling ? "Waiting for cancellation" : "Scanning folder"}
        />
      )}
      {awaitingTotals && (
        <div className="du-result__loading">
          <LiveStatus>
            <Text>Reading space totals…</Text>
          </LiveStatus>
          <Progress label="Reading space totals" />
        </div>
      )}
    </header>
  );
}

function ScanStatus({
  state = "queued",
  pending,
  focusTarget,
}: {
  state: JobState | "unavailable" | undefined;
  pending: ScanControl | undefined;
  focusTarget: Ref<HTMLDivElement>;
}) {
  const statusText =
    pending === "cancel" || state === "cancel_requested"
      ? "cancellation requested"
      : pending === "resume"
        ? "resume requested"
        : pending === "pause" || state === "pause_requested"
          ? "pause requested"
          : state;
  return (
    <LiveStatus ref={focusTarget} focusTarget>
      <StatusBadge tone={state === "done" ? "good" : "info"}>
        {state === "done" ? "Scan finished" : `Scan status: ${statusText}`}
      </StatusBadge>
    </LiveStatus>
  );
}

function ResultVersionSummary({
  total,
  hasNewResult,
  showLatest,
  switching,
  finished,
}: {
  total: SpaceTotal | undefined;
  hasNewResult: boolean;
  showLatest: () => void;
  switching: boolean;
  finished: boolean;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const summary = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState<{
    version: string;
    shown: boolean;
  }>();
  const version = total?.resultVersion;
  const waitingToSwitch = selection?.shown === false;

  useLayoutEffect(() => {
    if (!waitingToSwitch || hasNewResult || switching) return;
    if (!version || version === selection.version) {
      setSelection(undefined);
      return;
    }
    // Keep the action mounted through this render so we can check its focus
    // before removing it. Do not move focus if the user already moved on.
    if (button.current && button.current === document.activeElement)
      summary.current?.focus({ preventScroll: true });
    setSelection({ version, shown: true });
  }, [hasNewResult, selection, switching, version, waitingToSwitch]);

  const shownLatest = selection?.shown && selection.version === version;
  return (
    <div className="du-result-version" data-result={total ? "" : undefined}>
      <LiveStatus ref={summary} focusTarget={total !== undefined}>
        {total && (
          <Text kind="muted">
            Result version: {resultVersionLabels[total.completeness]}
          </Text>
        )}
        {hasNewResult && <Text>{newerResultMessage(finished, total)}</Text>}
        {!hasNewResult && shownLatest && (
          <Text>Now showing the latest result.</Text>
        )}
      </LiveStatus>
      {version && (hasNewResult || waitingToSwitch) && (
        <Action
          ref={button}
          isPending={switching}
          onPress={() => {
            setSelection({ version, shown: false });
            showLatest();
          }}
        >
          {switching ? "Reading latest result…" : "Show latest result"}
        </Action>
      )}
    </div>
  );
}

function newerResultMessage(
  finished: boolean,
  total: SpaceTotal | undefined,
): string {
  if (!finished)
    return "Newer result available. Showing the current result until you switch.";
  return total?.completeness === "partial"
    ? "You’re still viewing an interim result."
    : "You’re still viewing an earlier result.";
}

/** Displays the selected result without replacing it on later publications. */
function PinnedResult({
  client,
  total,
  roots,
}: {
  client: AnalysisClient;
  total: SpaceTotal;
  roots: readonly OsPath[];
}) {
  const [display, setDisplay] = useState(windowsSizeDisplay);
  const [basis, setBasis] = useState<SizeBasisFilter>("all");
  const sizes = total.sizes.filter(
    ({ size }) => basis === "all" || size.basis === basis,
  );
  const lead = sizes.find(({ size }) => size.basis === "logical") ?? sizes[0];
  const supporting = sizes.filter((entry) => entry !== lead);
  return (
    <div className="du-results">
      <div className="du-metrics">
        {(lead ? [lead, ...supporting] : supporting).map(
          ({ size, completeness }, index) => {
            const value = displaySize(size, display);
            const raw =
              size.state !== "unknown" && !value.endsWith(" bytes")
                ? formatBytes(size.bytes)
                : undefined;
            return (
              <Metric
                key={size.basis}
                label={sizeLabels[size.basis]}
                prominent={index === 0}
                detail={[displayMeasurement(size, completeness), raw]
                  .filter(Boolean)
                  .join(" · ")}
              >
                {value}
              </Metric>
            );
          },
        )}
      </div>
      <Coverage total={total} />
      <Readout
        label="Scanned paths"
        rows={[
          {
            label: "Scanned paths",
            value: roots.map((root) => root.display).join(", "),
          },
        ]}
      />
      <SizeDisplayControls
        basis={basis}
        display={display}
        onBasis={setBasis}
        onDisplay={setDisplay}
      />
      <VolumeExplanation
        key={`volume:${total.resultVersion}`}
        client={client}
        resultVersion={total.resultVersion}
        roots={roots}
        display={display}
      />
      <ScanIssues
        key={`issues:${total.resultVersion}`}
        client={client}
        resultVersion={total.resultVersion}
      />
      <ScopeDetails
        key={total.resultVersion}
        client={client}
        resultVersion={total.resultVersion}
        multiRoot={roots.length > 1}
        display={display}
      />
    </div>
  );
}

/** Shows only the observed attributes of the pinned scope root. */
function ScopeDetails({
  client,
  resultVersion,
  multiRoot,
  display,
}: {
  client: AnalysisClient;
  resultVersion: string;
  multiRoot: boolean;
  display: SizeDisplay;
}) {
  const [detail, setDetail] = useState<BrowseDetail>();
  const [pending, setPending] = useState(false);
  const [problem, setProblem] = useState<string>();

  if (multiRoot) return null;

  async function toggle() {
    if (detail) {
      setDetail(undefined);
      return;
    }
    setProblem(undefined);
    setPending(true);
    try {
      const response = await client.call("browse.detail", {
        resultVersion,
        item: { resultVersion, itemNo: 0 },
      });
      if (
        response.item.resultVersion !== resultVersion ||
        response.item.itemNo !== 0
      ) {
        setProblem("The result changed. Run a new scan to see folder details.");
        return;
      }
      setDetail(response);
    } catch (error) {
      setProblem(
        error instanceof AnalysisError && error.code === "STALE_VERSION"
          ? "The result changed. Run a new scan to see folder details."
          : "Folder details are unavailable.",
      );
    } finally {
      setPending(false);
    }
  }

  return (
    <section aria-label="Folder details">
      <Action isDisabled={pending} onPress={() => void toggle()}>
        {detail ? "Hide folder details" : "View folder details"}
      </Action>
      {pending && <Progress label="Loading folder details" />}
      {problem && <Notice tone="error">{problem}</Notice>}
      {detail && (
        <>
          <Readout
            label="Observed folder attributes"
            rows={[
              { label: "Folder", value: attribute(detail.displayPath) },
              { label: "Kind", value: attribute(detail.kind) },
              {
                label: "Logical size",
                value: attribute(detail.logicalSize, (value) =>
                  displaySizeWithBytes(value, display),
                ),
              },
              {
                label: "Allocated size",
                value: attribute(detail.allocatedSize, (value) =>
                  displaySizeWithBytes(value, display),
                ),
              },
              { label: "Modified", value: attribute(detail.modifiedAt) },
              { label: "Created", value: attribute(detail.createdAt) },
              {
                label: "Result published",
                value: attribute(detail.observedAt),
              },
              {
                label: "Source",
                value: detail.observationOnly ? "Scan record" : "Scan",
              },
            ]}
          />
          <ItemSpaceDetails
            client={client}
            item={detail.item}
            display={display}
          />
        </>
      )}
    </section>
  );
}

function attribute<T>(
  field: BrowseValue<T>,
  format: (value: T) => string = String,
): string {
  switch (field.state) {
    case "present":
      return format(field.value);
    case "absent":
      return "No value";
    case "unknown":
      return field.reasonKey === "space.data_insufficient"
        ? "Size was not established"
        : "Unavailable in this scan";
  }
}
