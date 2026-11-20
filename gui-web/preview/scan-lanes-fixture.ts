import { windowsPath } from "../src/ipc";
import type { ItemRef, ScanLaneStatus } from "../src/ipc";
import type { ScanPriorityArea } from "../src/state/scan-priority";

const priorityItem: ItemRef = {
  resultVersion: "0193f000-0000-7000-8000-000000000258",
  itemNo: 42,
};

export const scanLanePriorityArea: ScanPriorityArea = {
  jobRef: "preview-scan",
  item: priorityItem,
  name: "영상",
};

export const scanLaneRoots = [
  windowsPath("C:\\Users\\민서\\문서"),
  windowsPath("C:\\Users\\민서\\사진"),
  windowsPath("D:\\프로젝트\\활성 작업"),
  windowsPath("E:\\백업"),
];

const lanes: ScanLaneStatus["lanes"] = [
  {
    laneIndex: 0,
    rootIndexes: [0, 1],
    inFlight: 2,
    limit: 4,
    waitReason: "none",
    waitConfirmed: true,
    remainingDirectories: "1234",
    failures: "0",
  },
  {
    laneIndex: 1,
    rootIndexes: [2],
    priorityArea: priorityItem,
    remainingPrioritizedDirectories: "8",
    inFlight: 1,
    limit: 2,
    waitReason: "io_wait",
    waitConfirmed: true,
    remainingDirectories: "48",
    failures: "2",
  },
  {
    laneIndex: 2,
    rootIndexes: [3],
    inFlight: 0,
    limit: 3,
    waitReason: "backpressure",
    waitConfirmed: false,
    remainingDirectories: "9",
    failures: "1",
  },
];

export const scanLaneStatus: ScanLaneStatus = {
  sequence: 28,
  lanes,
  observedAt: "2026-11-20T09:00:00.000Z",
};

export const oneScanLaneStatus: ScanLaneStatus = {
  ...scanLaneStatus,
  sequence: 29,
  lanes: [lanes[1]!],
};
