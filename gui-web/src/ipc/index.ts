import {
  invokeNative,
  subscribeNative,
  subscribeNativeFolderDrop,
} from "./bridge";
import type {
  AnalysisJobControl,
  AnalysisJobRequest,
  AnalysisJobStatus,
  BrowseDetail,
  BrowseDetailRequest,
  BrowseOpenRequest,
  BrowseOpenResponse,
  CallEnvelope,
  EventEnvelope,
  HistoryDetail,
  HistoryDetailRequest,
  HistoryItemsRequest,
  HistoryItemsResponse,
  HistoryListRequest,
  HistoryListResponse,
  Id,
  OsPath,
  QueryPageRequest,
  QueryRegionOpenRequest,
  QueryRegionOpened,
  QueryRegionCloseRequest,
  QueryRegionClosed,
  QueryCancelRequest,
  QueryCancelled,
  QueryPageResponse,
  ResultEnvelope,
  ScanFailures,
  ScanFailuresRequest,
  ScanStart,
  ScanStartRequest,
  SettingsRestoreRequest,
  SettingsRestoreStatus,
  ScopeReview,
  ScopeReviewRequest,
  SpaceDetail,
  SpaceDetailRequest,
  SpaceTotal,
  SpaceTotalRequest,
  TargetList,
  TargetListRequest,
  VolumeGapRequest,
  VolumeGapResponse,
  VizLocate,
  VizLocateRequest,
  VizRows,
  VizRowsRequest,
} from "./generated/contracts";

export { SIZE_UNIT_RULES } from "./generated/contracts";
export type {
  AnalysisJobControl,
  AnalysisJobStatus,
  AnalysisTarget,
  BrowseDetail,
  BrowseOpenRequest,
  BrowseOpenResponse,
  FolderQueryPage,
  FolderQueryRow,
  ItemRef,
  QueryPageRequest,
  QueryPageResponse,
  BrowseValue,
  EventEnvelope,
  HistoryChangedBody,
  HistoryDetail,
  HistoryDetailRequest,
  HistoryItemOutcome,
  HistoryItemRow,
  HistoryItemsRequest,
  HistoryItemsResponse,
  HistoryEndState,
  HistoryListCounts,
  HistoryListFilter,
  HistoryListRequest,
  HistoryListResponse,
  HistoryListRevision,
  HistoryListRow,
  HistoryListSummary,
  HistoryRelation,
  Id,
  JobState,
  OsPath,
  ScanFailureKind,
  ScanFailureRow,
  ScanFailureStage,
  ScanFailures,
  ScanFailuresRequest,
  ScanStart,
  SettingsRestoreStatus,
  ScopeDraft,
  ScopeReview,
  SpaceTotal,
  SpaceTotalSize,
  SpaceDetail,
  SpaceDetailSize,
  SizeValue,
  TargetList,
  TargetProbeEvent,
  TargetStatusChanged,
  VolumeGapAmount,
  VolumeGapObservation,
  VolumeGapRequest,
  VolumeGapResponse,
  VizLocate,
  VizLocateRequest,
  VizRow,
  VizRows,
  VizRowsRequest,
} from "./generated/contracts";

type Commands = {
  "target.list": [TargetListRequest, TargetList];
  "scope.review": [ScopeReviewRequest, ScopeReview];
  "scan.start": [ScanStartRequest, ScanStart];
  "scan.failures": [ScanFailuresRequest, ScanFailures];
  "job.get": [AnalysisJobRequest, AnalysisJobStatus];
  "job.pause": [AnalysisJobRequest, AnalysisJobControl];
  "job.resume": [AnalysisJobRequest, AnalysisJobControl];
  "job.cancel": [AnalysisJobRequest, AnalysisJobControl];
  "history.list": [HistoryListRequest, HistoryListResponse];
  "history.get": [HistoryDetailRequest, HistoryDetail];
  "history.items": [HistoryItemsRequest, HistoryItemsResponse];
  "settings.restore_status": [SettingsRestoreRequest, SettingsRestoreStatus];
  "space.total": [SpaceTotalRequest, SpaceTotal];
  "space.detail": [SpaceDetailRequest, SpaceDetail];
  "space.volume_gap": [VolumeGapRequest, VolumeGapResponse];
  "browse.detail": [BrowseDetailRequest, BrowseDetail];
  "browse.open": [BrowseOpenRequest, BrowseOpenResponse];
  "query.region.open": [QueryRegionOpenRequest, QueryRegionOpened];
  "query.region.close": [QueryRegionCloseRequest, QueryRegionClosed];
  "query.cancel": [QueryCancelRequest, QueryCancelled];
  "query.page": [QueryPageRequest, QueryPageResponse];
  "viz.rows": [VizRowsRequest, VizRows];
  "viz.locate": [VizLocateRequest, VizLocate];
};

export interface AnalysisClient {
  call<K extends keyof Commands>(
    name: K,
    body: Commands[K][0],
  ): Promise<Commands[K][1]>;
  subscribe(listener: (event: EventEnvelope) => void): () => void;
  subscribeFolderDrop?(listener: (drop: FolderDrop) => void): () => void;
}

export type FolderDrop = { seq: number } & (
  | { pending: true }
  | { path: OsPath }
  | { reason: "invalid_target" | "unsupported" }
);

export class AnalysisError extends Error {
  constructor(
    readonly code: string,
    readonly reasonKey: string,
  ) {
    super(reasonKey);
  }
}

/** Creates the UUID v7 request identifier required by the host envelope. */
function requestId(): Id {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  let timestamp = BigInt(Date.now());
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = Number(timestamp & 255n);
    timestamp >>= 8n;
  }
  const view = new DataView(bytes.buffer);
  view.setUint8(6, (view.getUint8(6) & 15) | 112);
  view.setUint8(8, (view.getUint8(8) & 63) | 128);
  const hex = Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20),
  ].join("-");
}

type Transport = (
  command: string,
  args: { envelope: CallEnvelope },
) => Promise<ResultEnvelope>;

/** Connects analysis commands and events through the versioned desktop transport. */
export function createAnalysisClient(
  transport: Transport = invokeNative,
): AnalysisClient {
  return {
    async call<K extends keyof Commands>(
      name: K,
      body: Commands[K][0],
    ): Promise<Commands[K][1]> {
      // Preserve the requested revision even if the caller reuses its object.
      const requestedRevision =
        name === "viz.rows"
          ? (body as VizRowsRequest).resultRevision
          : undefined;
      const requestBody =
        name === "viz.rows" ? { ...(body as VizRowsRequest) } : body;
      const envelope = {
        name,
        body: requestBody,
        correlationId: requestId(),
        contractVersion: 1,
      };
      const result = await transport("du_call", { envelope });
      if (result.error)
        throw new AnalysisError(result.error.code, result.error.reasonKey);
      if (
        requestedRevision !== undefined &&
        (typeof result.body !== "object" ||
          result.body === null ||
          !("resultRevision" in result.body) ||
          result.body.resultRevision !== requestedRevision)
      ) {
        throw new Error("DiskUnfold IPC returned a mismatched result revision");
      }
      return result.body as Commands[K][1];
    },
    subscribe: subscribeNative,
    subscribeFolderDrop: (listener) =>
      subscribeNativeFolderDrop((drop) =>
        listener(
          "path" in drop
            ? { seq: drop.seq, path: windowsPath(drop.path) }
            : drop,
        ),
      ),
  };
}

/** Encodes a path typed by the user without normalizing or resolving it. */
export function windowsPath(input: string): OsPath {
  let bytes = "";
  for (let index = 0; index < input.length; index += 1) {
    const unit = input.charCodeAt(index);
    bytes += String.fromCharCode(unit & 255, unit >> 8);
  }
  const raw = btoa(bytes)
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replace(/=+$/, "");
  return { encoding: "windows_utf16le", raw, display: input, lossy: false };
}

export { hasWindowsAnalysisBridge } from "./bridge";
export type { CallEnvelope, ResultEnvelope } from "./generated/contracts";
export function createIpcClient(transport: Transport = invokeNative) {
  return {
    call(envelope: CallEnvelope) {
      return transport("du_call", { envelope });
    },
  };
}
