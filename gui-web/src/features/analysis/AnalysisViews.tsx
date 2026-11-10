import { useState } from "react";
import {
  Action,
  Choice,
  ChoiceGroup,
  FolderDropSurface,
  Heading,
  Icon,
  Metric,
  Notice,
  PathField,
  Progress,
  Readout,
  StatusBadge,
  Text,
} from "../../components";
import { AnalysisError, windowsPath } from "../../ipc";
import type {
  AnalysisClient,
  BrowseDetail,
  BrowseValue,
  OsPath,
  SizeValue,
  SpaceTotal,
} from "../../ipc";
import type { useScanResult } from "../../state/analysis-scan";
import type { useTargets } from "../../state/analysis-targets";

const sizeLabels = {
  logical: "Logical size",
  allocated: "Allocated size",
  unique_physical: "Unique physical size",
};

function formatBytes(bytes: string): string {
  return `${BigInt(bytes).toLocaleString()} bytes`;
}

function displaySize(size: SizeValue): string {
  if (size.state === "unknown") return "Unavailable";
  return `${size.state === "lower_bound" ? "At least " : ""}${formatBytes(size.bytes)}`;
}

/** Presents discovered drives and the direct path entry defined for scope review. */
export function TargetPicker({
  targets,
  disabled,
  selected,
  choose,
  addPath,
  removePath,
  dropEnabled,
  dropError,
  pathDescription,
}: {
  targets: ReturnType<typeof useTargets>;
  disabled: boolean;
  selected: (OsPath | undefined)[];
  choose: (path: OsPath | undefined, index?: number) => void;
  addPath: () => void;
  removePath: (index: number) => void;
  dropEnabled: boolean;
  dropError: string | undefined;
  pathDescription: string;
}) {
  const error = targets.error ?? targets.list?.unavailableReason;
  const notice =
    error === "os.unsupported"
      ? "Automatic drive listing is unavailable. You can still enter a folder below."
      : error === "os.call_slow"
        ? "Drive listing is taking longer than expected. Refresh later or enter a folder below."
        : error;
  const entries = targets.list?.targets ?? [];
  const pathField = (
    <PathField
      label="Windows folder path"
      description={pathDescription}
      placeholder={"C:\\Folder"}
      isDisabled={disabled}
      value={selected[0]?.display ?? ""}
      onChange={(path) => choose(path ? windowsPath(path) : undefined)}
    />
  );
  return (
    <>
      <div className="du-card__header">
        <div>
          <Text kind="eyebrow">01 · Target & scope</Text>
          <Heading level={3}>Choose what to scan</Heading>
        </div>
        <Action
          variant="quiet"
          icon="refresh"
          aria-label="Refresh targets"
          isDisabled={targets.loading || disabled}
          onPress={() => void targets.refresh(true)}
        />
      </div>
      {targets.loading && <Progress label="Loading targets" />}
      {notice && (
        <Notice
          tone={
            error === "os.unsupported" || error === "os.call_slow"
              ? "info"
              : "error"
          }
        >
          {notice}
        </Notice>
      )}
      {entries.length > 0 && (
        <ChoiceGroup
          label="Choose a target"
          value={selected[0]?.raw ?? null}
          isDisabled={disabled}
          onChange={(raw) =>
            choose(entries.find(({ path }) => path.raw === raw)?.path)
          }
        >
          {entries.map((target) => {
            const probe = targets.probes[target.path.raw];
            const access =
              probe?.access ??
              (targets.finished && target.access === "checking"
                ? "unknown"
                : target.access);
            const capacity = probe?.totalBytes;
            const kind =
              target.kind?.replaceAll("_", " ") ?? "Type unavailable";
            return (
              <Choice
                key={target.path.raw}
                value={target.path.raw}
                label={target.path.display}
                icon="drive"
                description={`${kind}${capacity ? ` · ${formatBytes(capacity)} total` : ""}`}
                status={{
                  text: access,
                  tone: access === "accessible" ? "good" : "neutral",
                }}
              />
            );
          })}
        </ChoiceGroup>
      )}
      <div className="du-primary-path">
        {dropEnabled ? (
          <FolderDropSurface isDisabled={disabled}>
            {pathField}
          </FolderDropSurface>
        ) : (
          pathField
        )}
        <Action isDisabled={disabled} onPress={addPath}>
          Add path
        </Action>
      </div>
      {selected.length > 1 && (
        <section className="du-scope-paths" aria-label="Additional paths">
          <Action isDisabled={disabled} onPress={() => removePath(0)}>
            Remove primary path
          </Action>
          {selected.slice(1).map((path, index) => (
            <div className="du-scope-path" key={index}>
              <PathField
                label={`Additional folder path ${index + 2}`}
                description="Enter a local folder or drive path."
                placeholder={"D:\\Folder"}
                isDisabled={disabled}
                value={path?.display ?? ""}
                onChange={(value) =>
                  choose(value ? windowsPath(value) : undefined, index + 1)
                }
              />
              <Action
                isDisabled={disabled}
                onPress={() => removePath(index + 1)}
              >
                {`Remove path ${index + 2}`}
              </Action>
            </div>
          ))}
        </section>
      )}
      {dropError && <Notice tone="error">{dropError}</Notice>}
    </>
  );
}

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
  multiRoot,
}: {
  client: AnalysisClient;
  scan: ReturnType<typeof useScanResult>;
  started: boolean;
  running: boolean;
  problem: string | undefined;
  multiRoot: boolean;
}) {
  const error = problem ?? scan.error ?? scan.status?.reasonKey;
  const state = scan.unavailable
    ? "unavailable"
    : (scan.status?.state ?? "queued");
  const awaitingTotals =
    !running &&
    Boolean(scan.status?.resultVersion) &&
    !scan.total &&
    !scan.error;
  return (
    <>
      <div className="du-card__header">
        <div>
          <Text kind="eyebrow">02 · Scan & results</Text>
          <Heading level={3}>Space totals</Heading>
        </div>
        {started && (
          <StatusBadge tone={state === "done" ? "good" : "info"} live>
            Scan status: {state}
          </StatusBadge>
        )}
      </div>
      {running && <Progress label="Scanning folder" />}
      {error && <Notice tone="error">{error}</Notice>}
      {awaitingTotals && <Progress label="Reading space totals" />}
      {scan.total ? (
        <PinnedResult
          client={client}
          total={scan.total}
          multiRoot={multiRoot}
          showLatest={scan.hasNewResult ? scan.showLatest : undefined}
        />
      ) : !awaitingTotals ? (
        <div className="du-empty">
          <Icon name="layers" />
          <Text>
            {running ? "Collecting file metadata" : "No result to display"}
          </Text>
          <Text kind="muted">
            {running
              ? "Totals appear when a result is ready."
              : "Review a folder and start a scan to see its space totals."}
          </Text>
        </div>
      ) : null}
    </>
  );
}

/** Displays the selected result without replacing it on later publications. */
function PinnedResult({
  client,
  total,
  multiRoot,
  showLatest,
}: {
  client: AnalysisClient;
  total: SpaceTotal;
  multiRoot: boolean;
  showLatest: (() => void) | undefined;
}) {
  return (
    <div className="du-results">
      <Text kind="muted">Result completeness: {total.completeness}</Text>
      {showLatest && (
        <Notice>
          <Text>New result available</Text>
          <Action onPress={showLatest}>Show latest result</Action>
        </Notice>
      )}
      <div className="du-metrics">
        {total.sizes.map((value) => (
          <Metric
            key={value.size.basis}
            label={sizeLabels[value.size.basis]}
            detail={
              value.size.evidence === "os_reported"
                ? "Reported by the file system"
                : undefined
            }
          >
            {displaySize(value.size)}
          </Metric>
        ))}
      </div>
      <Coverage total={total} />
      <ScopeDetails
        key={total.resultVersion}
        client={client}
        resultVersion={total.resultVersion}
        multiRoot={multiRoot}
      />
    </div>
  );
}

/** Shows only the observed attributes of the pinned scope root. */
function ScopeDetails({
  client,
  resultVersion,
  multiRoot,
}: {
  client: AnalysisClient;
  resultVersion: string;
  multiRoot: boolean;
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
        <Readout
          label="Observed folder attributes"
          rows={[
            { label: "Folder", value: attribute(detail.displayPath) },
            { label: "Kind", value: attribute(detail.kind) },
            {
              label: "Logical size",
              value: attribute(detail.logicalSize, displaySize),
            },
            {
              label: "Allocated size",
              value: attribute(detail.allocatedSize, displaySize),
            },
            { label: "Modified", value: attribute(detail.modifiedAt) },
            { label: "Created", value: attribute(detail.createdAt) },
            { label: "Observed", value: attribute(detail.observedAt) },
            {
              label: "Source",
              value: detail.observationOnly ? "Scan record" : "Scan",
            },
          ]}
        />
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
