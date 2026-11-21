import { useContext, useEffect, useId, useRef, useState } from "react";
import {
  Action,
  DataTable,
  DetailDisclosure,
  Heading,
  LiveStatus,
  MiddleEllipsisText,
  Text,
  withProgressEllipsis,
} from "../../components";
import type { LocaleText } from "../../i18n/presentation";
import { useLocaleText } from "../../i18n/presentation";
import { displayInteger, useRegionalFormat } from "../../i18n/regional-format";
import { AnalysisError } from "../../ipc";
import type {
  ItemRef,
  OsPath,
  ScanCollection,
  ScanLane,
  ScanLaneStatus,
  ScanResourceMode,
} from "../../ipc";
import type { ScanPriorityArea } from "../../state/scan-priority";
import {
  ResourceLimitDisclosure,
  ResourceModeSelector,
} from "./ResourceControls";
import type { ResourceLimitError } from "./ResourceControls";
import { ScanResourceContext } from "./scan-resource-context";
import type { ScanResourceOwner } from "./scan-resource-context";
import "../../theme/scan-lanes.css";

const ROW_INTERVAL_MS = 1_000;
type LaneRows = (rows: ScanLane[]) => void;
type PendingResource = { mode: ScanResourceMode; effectiveLimit: number };
type LaneWaitCause =
  "none" | "io_wait" | "backpressure" | "device_cap" | "unknown";

/** Keeps the first lane order and publishes each row no more than once a second. */
export function createScanLaneThrottle(onRows: LaneRows) {
  let initialized = false;
  let sequence = -1;
  let order: number[] = [];
  let rows = new Map<number, ScanLane>();
  const publishedAt = new Map<number, number>();
  const pending = new Map<number, ScanLane>();
  const timers = new Map<number, ReturnType<typeof setTimeout>>();

  function emit() {
    onRows(order.map((index) => rows.get(index)!));
  }

  function publish(lane: ScanLane) {
    rows.set(lane.laneIndex, lane);
    publishedAt.set(lane.laneIndex, Date.now());
    emit();
  }

  function update(status: ScanLaneStatus) {
    if (status.sequence <= sequence) return;
    sequence = status.sequence;
    if (!initialized) {
      initialized = true;
      const firstRows = [...status.lanes];
      order = firstRows.map((lane) => lane.laneIndex);
      rows = new Map(firstRows.map((lane) => [lane.laneIndex, lane]));
      const now = Date.now();
      for (const index of order) publishedAt.set(index, now);
      emit();
      return;
    }

    const now = Date.now();
    for (const lane of status.lanes) {
      if (!rows.has(lane.laneIndex)) continue;
      const current = rows.get(lane.laneIndex)!;
      if (lane.mode !== current.mode || lane.limit !== current.limit) {
        pending.delete(lane.laneIndex);
        publish(lane);
        continue;
      }
      const last = publishedAt.get(lane.laneIndex) ?? now;
      const delay = ROW_INTERVAL_MS - (now - last);
      if (delay <= 0) {
        pending.delete(lane.laneIndex);
        publish(lane);
      } else {
        pending.set(lane.laneIndex, lane);
        if (!timers.has(lane.laneIndex)) {
          timers.set(
            lane.laneIndex,
            setTimeout(() => {
              timers.delete(lane.laneIndex);
              const latest = pending.get(lane.laneIndex);
              pending.delete(lane.laneIndex);
              if (latest) publish(latest);
            }, delay),
          );
        }
      }
    }
  }

  return {
    update,
    dispose() {
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      pending.clear();
    },
  };
}

function waitCause(lane: ScanLane): LaneWaitCause {
  if (!lane.waitConfirmed) return "unknown";
  switch (String(lane.waitReason)) {
    case "none":
      return "none";
    case "io_wait":
      return "io_wait";
    case "backpressure":
      return "backpressure";
    case "device_cap":
      return "device_cap";
    default:
      return "unknown";
  }
}

function laneStatusText(lane: ScanLane, text: LocaleText) {
  const cause = waitCause(lane);
  const message = {
    none: ["ui.analysis.lanes_reading", "Reading…"],
    io_wait: ["ui.analysis.lanes_wait_io", "Waiting for the disk…"],
    backpressure: [
      "ui.analysis.lanes_wait_backpressure",
      "Waiting for processing to catch up…",
    ],
    device_cap: [
      "ui.analysis.lanes_wait_device_cap",
      "Waiting for other reads on this device…",
    ],
    unknown: ["ui.analysis.lanes_wait_unknown", "Waiting…"],
  }[cause] as [string, string];
  return withProgressEllipsis(text(message[0], message[1]));
}

function laneIsIdle(lane: ScanLane) {
  return lane.inFlight === 0 && waitCause(lane) === "none";
}

function folderCount(lane: ScanLane, text: LocaleText) {
  return BigInt(lane.remainingDirectories) === 0n && laneIsIdle(lane)
    ? text("ui.analysis.lanes_done", "Done")
    : text("ui.analysis.lanes_folders_left", "{count}", {
        count: BigInt(lane.remainingDirectories),
      });
}

function priorityName(
  lane: ScanLane,
  jobRef: string,
  requested: ScanPriorityArea | undefined,
) {
  const area = lane.priorityArea;
  return requested?.jobRef === jobRef && area && sameItem(area, requested.item)
    ? requested.name
    : undefined;
}

function sameItem(left: ItemRef, right: ItemRef) {
  return (
    left.resultVersion === right.resultVersion && left.itemNo === right.itemNo
  );
}

function isControlUnavailable(failure: unknown) {
  return (
    failure instanceof AnalysisError &&
    (failure.reasonKey === "job.not_found" ||
      failure.reasonKey === "job.control_unsupported")
  );
}

function isResourceLimitRejected(failure: unknown) {
  return (
    failure instanceof AnalysisError &&
    failure.reasonKey === "scan.resource_limit_invalid"
  );
}

function isValidResourceLimit(value: string, max: number | undefined) {
  const parsed = Number(value);
  return (
    Number.isSafeInteger(parsed) &&
    parsed >= 1 &&
    (max === undefined || parsed <= max)
  );
}

function ownsActiveScan(
  owner: ScanResourceOwner | undefined,
  jobRef: string,
  hidden: boolean,
) {
  return owner?.active === true && owner.jobRef === jobRef && !hidden;
}

function resourceFeedbackText(
  feedback: "applying" | "accepted" | "failed" | undefined,
  text: LocaleText,
) {
  if (feedback === "applying")
    return text("ui.analysis.resource_applying", "Applying");
  if (feedback === "failed")
    return text(
      "ui.analysis.resource_change_failed",
      "Resource use could not be changed.",
    );
  return "";
}

function ResourceControlPanel({
  visible,
  resource,
  limitDraft,
  maxPermit,
  limitError,
  limitErrorPrevious,
  feedback,
  announcement,
  onModeChange,
  onLimitChange,
}: {
  visible: boolean;
  resource: { resourceMode: ScanResourceMode; userLimit: number | undefined };
  limitDraft: string;
  maxPermit: number | undefined;
  limitError: ResourceLimitError | undefined;
  limitErrorPrevious: number | undefined;
  feedback: "applying" | "accepted" | "failed" | undefined;
  announcement: string;
  onModeChange: (mode: ScanResourceMode) => void;
  onLimitChange: (value: string) => void;
}) {
  const text = useLocaleText();
  const feedbackText = resourceFeedbackText(feedback, text);
  return (
    <div hidden={!visible} className="du-scan-resource-control">
      <ResourceModeSelector
        mode={resource.resourceMode}
        onChange={onModeChange}
        showHelp={false}
      />
      <ResourceLimitDisclosure
        value={resource.userLimit}
        draft={limitDraft}
        max={maxPermit}
        error={limitError}
        previousValue={limitError ? limitErrorPrevious : resource.userLimit}
        onChange={onLimitChange}
      />
      {feedbackText && (
        <LiveStatus>
          <Text className="du-scan-resource-control__feedback">
            {feedbackText}
          </Text>
        </LiveStatus>
      )}
      <div className="du-announcer">
        <LiveStatus>
          <Text>{announcement}</Text>
        </LiveStatus>
      </div>
    </div>
  );
}

function priorityText(name: string | undefined, text: LocaleText) {
  return name
    ? text("ui.analysis.lanes_priority_area", "Analyzing first: {name}", {
        name,
      })
    : undefined;
}

function rootTitle(
  lane: ScanLane,
  roots: readonly OsPath[],
  text: LocaleText,
  name: string | undefined,
) {
  const rootIndex = lane.rootIndexes[0];
  const path = rootIndex === undefined ? undefined : roots[rootIndex]?.display;
  const total = BigInt(lane.rootCount ?? String(lane.rootIndexes.length));
  const otherRoots = total > 0n ? total - 1n : 0n;
  const areaLabel = priorityText(name, text);
  return (
    <div className="du-scan-lane__root-title">
      <div className="du-scan-lane__root-line">
        {path ? (
          <div className="du-scan-lane__path">
            <MiddleEllipsisText text={path} cell />
          </div>
        ) : (
          <div>
            {text(
              "ui.analysis.lanes_root_unavailable",
              "Scope path unavailable",
            )}
          </div>
        )}
        {otherRoots > 0n && (
          <div className="du-scan-lane__more">
            {text("ui.analysis.lanes_and_more", "and {count} more", {
              count: otherRoots,
            })}
          </div>
        )}
      </div>
      {areaLabel && <div className="du-scan-lane__priority">{areaLabel}</div>}
    </div>
  );
}

function laneCells(
  lane: ScanLane,
  jobRef: string,
  roots: readonly OsPath[],
  text: LocaleText,
  regional: ReturnType<typeof useRegionalFormat>,
  requested: ScanPriorityArea | undefined,
  pendingResources: ReadonlyMap<number, PendingResource>,
) {
  const failures = BigInt(lane.failures);
  const name = priorityName(lane, jobRef, requested);
  return [
    <DetailDisclosure key="storage" title={rootTitle(lane, roots, text, name)}>
      <Text>
        {text(
          "ui.analysis.lanes_reads_detail",
          "Reads in progress · {inFlight} of {limit}",
          { inFlight: BigInt(lane.inFlight), limit: BigInt(lane.limit) },
        )}
      </Text>
      {waitCause(lane) === "unknown" && (
        <Text>
          {text(
            "ui.analysis.lanes_wait_unconfirmed",
            "Wait cause · Not confirmed",
          )}
        </Text>
      )}
    </DetailDisclosure>,
    <div className="du-scan-lane__status" key="status">
      {laneStatusText(lane, text)}
      {pendingResources.has(lane.laneIndex) && (
        <div className="du-scan-lane__resource-pending">
          {text(
            "ui.analysis.resource_takes_effect",
            "Takes effect after the current folder",
          )}
        </div>
      )}
    </div>,
    <div className="du-scan-lane__number" key="folders">
      {folderCount(lane, text)}
    </div>,
    <div className="du-scan-lane__number" key="failures">
      {failures === 0n ? "" : displayInteger(failures, regional)}
    </div>,
  ];
}

export function ScanLaneProgress({
  jobRef,
  status,
  roots,
  priorityArea,
}: {
  jobRef: string;
  status: ScanLaneStatus;
  roots: readonly OsPath[];
  priorityArea?: ScanPriorityArea | undefined;
}) {
  const text = useLocaleText();
  const regional = useRegionalFormat();
  const owner = useContext(ScanResourceContext);
  const headingId = useId();
  const [rows, setRows] = useState<ScanLane[]>([]);
  const [announceIdle, setAnnounceIdle] = useState(false);
  const initialResource = resourceFromCollection(
    owner?.jobRef === jobRef ? owner.collection : undefined,
  );
  const [resource, setResource] = useState(initialResource);
  const [limitDraft, setLimitDraft] = useState(
    initialResource.userLimit === undefined
      ? ""
      : String(initialResource.userLimit),
  );
  const [limitError, setLimitError] = useState<ResourceLimitError>();
  const [limitErrorPrevious, setLimitErrorPrevious] = useState<
    number | undefined
  >();
  const [feedback, setFeedback] = useState<
    "applying" | "accepted" | "failed" | undefined
  >();
  const [announcement, setAnnouncement] = useState("");
  const [pendingResources, setPendingResources] = useState<
    Map<number, PendingResource>
  >(() => new Map());
  const [resourceHidden, setResourceHidden] = useState(false);
  const [expanded, setExpanded] = useState(true);
  const sequence = useRef(0);
  const resourceRef = useRef(initialResource);
  const acceptedResource = useRef(initialResource);
  const pendingMode = useRef<ScanResourceMode>(initialResource.resourceMode);
  const previousIdle = useRef<boolean | undefined>(undefined);
  const throttle = useRef<
    ReturnType<typeof createScanLaneThrottle> | undefined
  >(undefined);

  useEffect(() => {
    setRows([]);
    previousIdle.current = undefined;
    setAnnounceIdle(false);
    const initial = resourceFromCollection(
      owner?.jobRef === jobRef ? owner.collection : undefined,
    );
    resourceRef.current = initial;
    acceptedResource.current = initial;
    pendingMode.current = initial.resourceMode;
    sequence.current = 0;
    setResource(initial);
    setLimitDraft(
      initial.userLimit === undefined ? "" : String(initial.userLimit),
    );
    setLimitError(undefined);
    setLimitErrorPrevious(undefined);
    setFeedback(undefined);
    setAnnouncement("");
    setPendingResources(new Map());
    setResourceHidden(false);
    setExpanded(true);
    const current = createScanLaneThrottle(setRows);
    throttle.current = current;
    return () => {
      current.dispose();
      if (throttle.current === current) throttle.current = undefined;
    };
  }, [jobRef]);

  useEffect(() => {
    throttle.current?.update(status);
  }, [status]);

  useEffect(() => {
    if (rows.length === 0) return;
    const idle = rows.every(laneIsIdle);
    if (previousIdle.current === false && idle) setAnnounceIdle(true);
    else if (!idle) setAnnounceIdle(false);
    previousIdle.current = idle;
  }, [rows]);

  useEffect(() => {
    if (pendingResources.size === 0) return;
    const byIndex = new Map(rows.map((lane) => [lane.laneIndex, lane]));
    const remaining = new Map(
      [...pendingResources].filter(([laneIndex, expected]) => {
        const lane = byIndex.get(laneIndex);
        return (
          !lane ||
          lane.mode !== expected.mode ||
          lane.limit !== expected.effectiveLimit
        );
      }),
    );
    if (remaining.size === pendingResources.size) return;
    setPendingResources(remaining);
    if (remaining.size === 0) {
      setFeedback(undefined);
      setAnnouncement(resourceChangedText(pendingMode.current, text));
    }
  }, [pendingResources, rows, text]);

  const maxPermit = status.lanes.reduce<number | undefined>(
    (maximum, lane) =>
      lane.permit === undefined
        ? maximum
        : Math.max(maximum ?? lane.permit, lane.permit),
    undefined,
  );
  const resourceControlVisible = ownsActiveScan(owner, jobRef, resourceHidden);

  async function sendResource(
    next: { resourceMode: ScanResourceMode; userLimit: number | undefined },
    requestSequence: number,
    draft: string,
    keepInputError: boolean,
  ) {
    if (!owner || !resourceControlVisible) return;
    const before = acceptedResource.current;
    resourceRef.current = next;
    setResource(next);
    if (!keepInputError) {
      setLimitError(undefined);
      setLimitErrorPrevious(undefined);
    }
    setFeedback("applying");
    setAnnouncement("");
    pendingMode.current = next.resourceMode;
    setPendingResources(
      new Map(
        status.lanes.map((lane) => [
          lane.laneIndex,
          { mode: next.resourceMode, effectiveLimit: -1 },
        ]),
      ),
    );
    const call = (value: typeof next) =>
      owner.client.call("scan.set_resource", {
        jobRef,
        resourceMode: value.resourceMode,
        ...(value.userLimit === undefined
          ? {}
          : { userLimit: value.userLimit }),
      });
    const acceptResponse = (
      response: Awaited<ReturnType<typeof call>>,
      applied: typeof next,
    ) => {
      if (response.outcome === "already_terminal") {
        setResourceHidden(true);
        setFeedback(undefined);
        setPendingResources(new Map());
        return;
      }
      acceptedResource.current = applied;
      resourceRef.current = applied;
      setResource(applied);
      setLimitErrorPrevious(applied.userLimit);
      if (response.outcome === "no_effect") {
        setFeedback(undefined);
        setPendingResources(new Map());
        return;
      }
      const visibleLanes = new Set(status.lanes.map((lane) => lane.laneIndex));
      const pending = new Map(
        response.lanes
          .filter((lane) => visibleLanes.has(lane.laneIndex))
          .map(
            (lane) =>
              [
                lane.laneIndex,
                {
                  mode: applied.resourceMode,
                  effectiveLimit: lane.effectiveLimit,
                },
              ] as const,
          ),
      );
      pendingMode.current = applied.resourceMode;
      setPendingResources(pending);
      setFeedback(pending.size === 0 ? undefined : "accepted");
      if (pending.size === 0)
        setAnnouncement(resourceChangedText(applied.resourceMode, text));
    };
    try {
      const response = await call(next);
      if (requestSequence !== sequence.current) return;
      acceptResponse(response, next);
    } catch (failure) {
      if (requestSequence !== sequence.current) return;
      if (isControlUnavailable(failure)) {
        setResourceHidden(true);
        setFeedback(undefined);
        setPendingResources(new Map());
        return;
      }
      if (isResourceLimitRejected(failure)) {
        setLimitError("rejected");
        const fallback = {
          resourceMode: next.resourceMode,
          userLimit:
            next.userLimit === before.userLimit &&
            before.userLimit !== undefined
              ? undefined
              : before.userLimit,
        };
        setLimitErrorPrevious(fallback.userLimit);
        resourceRef.current = fallback;
        setResource(fallback);
        setLimitDraft(draft);
        try {
          const response = await call(fallback);
          if (requestSequence !== sequence.current) return;
          acceptResponse(response, fallback);
        } catch (retryFailure) {
          if (requestSequence !== sequence.current) return;
          if (isControlUnavailable(retryFailure)) {
            setResourceHidden(true);
            setFeedback(undefined);
            setPendingResources(new Map());
            return;
          }
          resourceRef.current = before;
          acceptedResource.current = before;
          setResource(before);
          setLimitErrorPrevious(before.userLimit);
          setFeedback("failed");
          setPendingResources(new Map());
        }
        return;
      }
      resourceRef.current = before;
      setResource(before);
      setLimitDraft(draft);
      setPendingResources(new Map());
      setFeedback("failed");
    }
  }

  function changeMode(mode: ScanResourceMode) {
    const requestSequence = ++sequence.current;
    const next = {
      ...resourceRef.current,
      resourceMode: mode,
      ...(limitError ? { userLimit: acceptedResource.current.userLimit } : {}),
    };
    void sendResource(
      next,
      requestSequence,
      limitDraft,
      limitError !== undefined,
    );
  }

  function changeLimit(value: string) {
    setLimitDraft(value);
    if (value === "") {
      const requestSequence = ++sequence.current;
      setFeedback(undefined);
      const next = { ...resourceRef.current, userLimit: undefined };
      void sendResource(next, requestSequence, value, false);
      return;
    }
    if (!isValidResourceLimit(value, maxPermit)) {
      setLimitError("invalid");
      setLimitErrorPrevious(acceptedResource.current.userLimit);
      return;
    }
    const requestSequence = ++sequence.current;
    setFeedback(undefined);
    const next = {
      ...resourceRef.current,
      userLimit: Number(value),
    };
    void sendResource(next, requestSequence, value, false);
  }

  if (rows.length === 0) return null;
  const totalLanes = BigInt(status.laneCount ?? String(rows.length));
  const singlePriority = priorityText(
    priorityName(rows[0]!, jobRef, priorityArea),
    text,
  );
  return (
    <div className="du-scan-lanes">
      <div className="du-scan-lanes__heading">
        {totalLanes === 1n ? (
          <Heading level={3}>
            {text("ui.analysis.lanes_heading", "Storage progress")}
          </Heading>
        ) : (
          <Action
            variant="quiet"
            aria-expanded={expanded}
            {...(expanded ? { "aria-controls": headingId } : {})}
            onPress={() => setExpanded((current) => !current)}
          >
            {expanded ? "▾ " : "▸ "}
            {text("ui.analysis.lanes_heading", "Storage progress")}
          </Action>
        )}
        <ResourceControlPanel
          visible={resourceControlVisible}
          resource={resource}
          limitDraft={limitDraft}
          maxPermit={maxPermit}
          limitError={limitError}
          limitErrorPrevious={limitErrorPrevious}
          feedback={feedback}
          announcement={announcement}
          onModeChange={changeMode}
          onLimitChange={changeLimit}
        />
      </div>
      {totalLanes === 1n ? (
        <div className="du-scan-lanes__single">
          <div className="du-scan-lane__status">
            {laneStatusText(rows[0]!, text)}
            {pendingResources.has(rows[0]!.laneIndex) && (
              <div className="du-scan-lane__resource-pending">
                {text(
                  "ui.analysis.resource_takes_effect",
                  "Takes effect after the current folder",
                )}
              </div>
            )}
          </div>
          {singlePriority && (
            <div className="du-scan-lane__priority">{singlePriority}</div>
          )}
        </div>
      ) : (
        <div id={headingId} hidden={!expanded}>
          <DataTable
            label={text("ui.analysis.lanes_heading", "Storage progress")}
            columns={[
              text("ui.analysis.lanes_storage", "Storage"),
              text("ui.analysis.lanes_status", "Status"),
              text("ui.analysis.lanes_folders_column", "Folders left"),
              text("ui.analysis.lanes_not_read", "Not read"),
            ]}
            rows={rows}
            density="compact"
            flat
            getRowKey={(lane) => String(lane.laneIndex)}
            renderCells={(lane) =>
              laneCells(
                lane,
                jobRef,
                roots,
                text,
                regional,
                priorityArea,
                pendingResources,
              )
            }
          />
        </div>
      )}
      {announceIdle && (
        <LiveStatus>
          <Text>
            {text(
              "ui.analysis.lanes_idle_announcement",
              "All storage lanes are idle.",
            )}
          </Text>
        </LiveStatus>
      )}
    </div>
  );
}

function resourceFromCollection(collection: ScanCollection | undefined) {
  return {
    resourceMode: collection?.resourceMode ?? ("auto" as const),
    userLimit: collection?.userLimit,
  };
}

function resourceChangedText(
  mode: ScanResourceMode,
  text: ReturnType<typeof useLocaleText>,
) {
  const name = {
    auto: text("ui.analysis.resource_auto", "Auto"),
    quiet: text("ui.analysis.resource_quiet", "Quiet"),
    fast: text("ui.analysis.resource_fast", "Fast"),
  }[mode];
  return text(
    "ui.analysis.resource_changed",
    "Resource use changed to {mode}",
    {
      mode: name,
    },
  );
}
