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
import { windowsPath } from "../../ipc";
import type { OsPath, SpaceTotal, SpaceTotalSize } from "../../ipc";
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

function displaySize({ size }: SpaceTotalSize): string {
  if (size.state === "unknown") return "Unavailable";
  return `${size.state === "lower_bound" ? "At least " : ""}${formatBytes(size.bytes)}`;
}

/** Presents discovered drives and the direct path entry defined for scope review. */
export function TargetPicker({
  targets,
  disabled,
  selected,
  choose,
  dropEnabled,
  dropError,
  pathDescription,
}: {
  targets: ReturnType<typeof useTargets>;
  disabled: boolean;
  selected: OsPath | undefined;
  choose: (path: OsPath | undefined) => void;
  dropEnabled: boolean;
  dropError: string | undefined;
  pathDescription: string;
}) {
  const error = targets.error ?? targets.list?.unavailableReason;
  const notice =
    error === "os.unsupported"
      ? "Automatic drive listing is unavailable. You can still enter a folder below."
      : error;
  const entries = targets.list?.targets ?? [];
  const pathField = (
    <PathField
      label="Windows folder path"
      description={pathDescription}
      placeholder={"C:\\Folder"}
      isDisabled={disabled}
      value={selected?.display ?? ""}
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
        <Notice tone={error === "os.unsupported" ? "info" : "error"}>
          {notice}
        </Notice>
      )}
      {entries.length > 0 && (
        <ChoiceGroup
          label="Choose a target"
          value={selected?.raw ?? null}
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
            return (
              <Choice
                key={target.path.raw}
                value={target.path.raw}
                label={target.path.display}
                icon="drive"
                description={`${target.kind.replaceAll("_", " ")}${capacity ? ` · ${formatBytes(capacity)} total` : ""}`}
                status={{
                  text: access,
                  tone: access === "accessible" ? "good" : "neutral",
                }}
              />
            );
          })}
        </ChoiceGroup>
      )}
      {dropEnabled ? (
        <FolderDropSurface isDisabled={disabled}>{pathField}</FolderDropSurface>
      ) : (
        pathField
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
  scan,
  started,
  running,
  problem,
}: {
  scan: ReturnType<typeof useScanResult>;
  started: boolean;
  running: boolean;
  problem: string | undefined;
}) {
  const error = problem ?? scan.error ?? scan.status?.reasonKey;
  const state = scan.unavailable
    ? "unavailable"
    : (scan.status?.state ?? "queued");
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
      {scan.total ? (
        <div className="du-results">
          <Text kind="muted">
            Result completeness: {scan.total.completeness}
          </Text>
          <div className="du-metrics">
            {scan.total.sizes.map((value) => (
              <Metric
                key={value.size.basis}
                label={sizeLabels[value.size.basis]}
                detail={
                  value.size.evidence === "os_reported"
                    ? "Reported by the file system"
                    : undefined
                }
              >
                {displaySize(value)}
              </Metric>
            ))}
          </div>
          <Coverage total={scan.total} />
        </div>
      ) : (
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
      )}
    </>
  );
}
