import { useEffect, useRef, useState } from "react";
import {
  DataTable,
  DetailDisclosure,
  LiveStatus,
  MiddleEllipsisText,
  Text,
  withProgressEllipsis,
} from "../../components";
import type { LocaleText } from "../../i18n/presentation";
import { useLocaleText } from "../../i18n/presentation";
import { displayInteger, useRegionalFormat } from "../../i18n/regional-format";
import type { ItemRef, OsPath, ScanLane, ScanLaneStatus } from "../../ipc";
import type { ScanPriorityArea } from "../../state/scan-priority";
import "../../theme/scan-lanes.css";

const ROW_INTERVAL_MS = 1_000;
type LaneRows = (rows: ScanLane[]) => void;
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
  const [rows, setRows] = useState<ScanLane[]>([]);
  const [announceIdle, setAnnounceIdle] = useState(false);
  const previousIdle = useRef<boolean | undefined>(undefined);
  const throttle = useRef<
    ReturnType<typeof createScanLaneThrottle> | undefined
  >(undefined);

  useEffect(() => {
    setRows([]);
    previousIdle.current = undefined;
    setAnnounceIdle(false);
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

  if (rows.length === 0) return null;
  const totalLanes = BigInt(status.laneCount ?? String(rows.length));
  const singlePriority = priorityText(
    priorityName(rows[0]!, jobRef, priorityArea),
    text,
  );
  return (
    <div className="du-scan-lanes">
      {totalLanes === 1n ? (
        <div className="du-scan-lanes__single">
          {laneStatusText(rows[0]!, text)}
          {singlePriority && (
            <div className="du-scan-lane__priority">{singlePriority}</div>
          )}
        </div>
      ) : (
        <DetailDisclosure
          title={text("ui.analysis.lanes_heading", "Storage progress")}
          defaultExpanded
        >
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
              laneCells(lane, jobRef, roots, text, regional, priorityArea)
            }
          />
        </DetailDisclosure>
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
