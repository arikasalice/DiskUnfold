import {
  hasRecentScopes,
  invokeNative,
  subscribeNative,
  subscribeNativeFolderDrop,
} from "./bridge";
import type {
  AnalysisJobControl,
  AnalysisJobListRequest,
  AnalysisJobListRow,
  AnalysisJobRequest,
  AnalysisJobStatus,
  BrowseCopyPathOutcome,
  BrowseCopyPathRequest,
  BrowseDetail,
  BrowseDetailRequest,
  BrowseOpenRequest,
  BrowseOpenResponse,
  CallEnvelope,
  DisplayPreferences,
  DisplayPreferencesRequest,
  EventEnvelope,
  HistoryDetail,
  HistoryDetailRequest,
  HistoryItemsRequest,
  HistoryItemsResponse,
  HistoryListRequest,
  HistoryListResponse,
  Id,
  LocaleGetProfileRequest,
  LocaleBundleRequest,
  LocaleBundleResponse,
  LocaleProfile,
  LocaleProfileSetRequest,
  LocaleReportMissingRequest,
  LocaleReportMissingResponse,
  OsPath,
  QueryPageRequest,
  QueryRegionOpenRequest,
  QueryRegionOpened,
  QueryRegionCloseRequest,
  QueryRegionClosed,
  QueryCancelRequest,
  QueryCancelled,
  QueryPageResponse,
  RecentScopeList,
  RecentScopeListRequest,
  RecentScopeOpen,
  RecentScopeOpenRequest,
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
  VizAvailabilityRequest,
  VizAvailabilityResponse,
  VizCancelRequest,
  VizCancelResponse,
  VizCatalog,
  VizCatalogRequest,
  VizGroupRowsRequest,
  VizGroupRows,
  VizLocate,
  VizLocateRequest,
  VizRenderRequest,
  VizRenderResponse,
  VizViewChannel,
  VizViewOpenRequest,
  VizRows,
  VizRowsRequest,
} from "./generated/contracts";

export { SIZE_UNIT_RULES } from "./generated/contracts";
export type {
  AnalysisJobControl,
  AnalysisJobListRow,
  AnalysisJobStatus,
  AnalysisTarget,
  BrowseCopyPathOutcome,
  BrowseCopyPathRequest,
  BrowseDetail,
  BrowseOpenRequest,
  BrowseOpenResponse,
  FolderQueryPage,
  FolderQueryRow,
  ItemRef,
  QueryPageRequest,
  QueryPageResponse,
  BrowseValue,
  DisplayPreferences,
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
  LocaleAppScale,
  LocaleBundleBody,
  LocaleChangedBody,
  LocaleMissingEntry,
  LocaleProfile,
  LocaleProfileChanges,
  OsPath,
  RecentScopeList,
  RecentScopeListRow,
  RecentScopeOpen,
  RecentScopeOpenTarget,
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
  VizAvailabilityRequest,
  VizAvailabilityResponse,
  VizCancelRequest,
  VizCancelResponse,
  VizCatalog,
  VizCatalogRequest,
  VizGroupRowsRequest,
  VizGroupRows,
  VizCancelTarget,
  VizControlResult,
  VizGeometry,
  VizLegendEntry,
  VizLocate,
  VizLocateRequest,
  VizModel,
  VizQuantity,
  VizRelationship,
  VizRenderRequest,
  VizRenderResponse,
  VizRow,
  VizRows,
  VizRowsRequest,
  VizSizeRange,
  VizShape,
  VizSourceRef,
  VizViewChannel,
  VizViewOpenRequest,
} from "./generated/contracts";

type Commands = {
  "target.list": [TargetListRequest, TargetList];
  "scope.review": [ScopeReviewRequest, ScopeReview];
  "scope.recent_list": [RecentScopeListRequest, RecentScopeList];
  "scope.recent_open": [RecentScopeOpenRequest, RecentScopeOpen];
  "scan.start": [ScanStartRequest, ScanStart];
  "scan.failures": [ScanFailuresRequest, ScanFailures];
  "job.get": [AnalysisJobRequest, AnalysisJobStatus];
  "job.list": [AnalysisJobListRequest, AnalysisJobListRow[]];
  "job.pause": [AnalysisJobRequest, AnalysisJobControl];
  "job.resume": [AnalysisJobRequest, AnalysisJobControl];
  "job.cancel": [AnalysisJobRequest, AnalysisJobControl];
  "history.list": [HistoryListRequest, HistoryListResponse];
  "history.get": [HistoryDetailRequest, HistoryDetail];
  "history.items": [HistoryItemsRequest, HistoryItemsResponse];
  "settings.restore_status": [SettingsRestoreRequest, SettingsRestoreStatus];
  "i18n.get_profile": [LocaleGetProfileRequest, LocaleProfile];
  "i18n.get_bundle": [LocaleBundleRequest, LocaleBundleResponse];
  "i18n.report_missing": [
    LocaleReportMissingRequest,
    LocaleReportMissingResponse,
  ];
  "i18n.set_profile": [LocaleProfileSetRequest, LocaleProfile];
  "space.display_preferences": [DisplayPreferencesRequest, DisplayPreferences];
  "space.total": [SpaceTotalRequest, SpaceTotal];
  "space.detail": [SpaceDetailRequest, SpaceDetail];
  "space.volume_gap": [VolumeGapRequest, VolumeGapResponse];
  "browse.detail": [BrowseDetailRequest, BrowseDetail];
  "browse.copy_path": [BrowseCopyPathRequest, BrowseCopyPathOutcome];
  "browse.open": [BrowseOpenRequest, BrowseOpenResponse];
  "query.region.open": [QueryRegionOpenRequest, QueryRegionOpened];
  "query.region.close": [QueryRegionCloseRequest, QueryRegionClosed];
  "query.cancel": [QueryCancelRequest, QueryCancelled];
  "query.page": [QueryPageRequest, QueryPageResponse];
  "viz.view.open": [VizViewOpenRequest, VizViewChannel];
  "viz.view.close": [VizViewChannel, VizViewOpenRequest];
  "viz.render": [VizRenderRequest, VizRenderResponse];
  "viz.availability": [VizAvailabilityRequest, VizAvailabilityResponse];
  "viz.catalog": [VizCatalogRequest, VizCatalog];
  "viz.cancel": [VizCancelRequest, VizCancelResponse];
  "viz.group_rows": [VizGroupRowsRequest, VizGroupRows];
  "viz.rows": [VizRowsRequest, VizRows];
  "viz.locate": [VizLocateRequest, VizLocate];
};

export interface AnalysisClient {
  readonly recentScopesEnabled?: boolean;
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

/** Carries a command failure while preserving retryability when the host knows it. */
export class AnalysisError extends Error {
  constructor(
    readonly code: string,
    readonly reasonKey: string,
    /** Retryability from the native error envelope, when available. */
    readonly retryable?: boolean,
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
    recentScopesEnabled: hasRecentScopes(),
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
        throw new AnalysisError(
          result.error.code,
          result.error.reasonKey,
          result.error.retryable,
        );
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

/** Preserves native UTF-16 code units and supplies a JSON-safe display string. */
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
  // Unicode matching treats a surrogate pair as one supplementary code point.
  const display = input.replace(/[\uD800-\uDFFF]/gu, "\uFFFD");
  return {
    encoding: "windows_utf16le",
    raw,
    display,
    lossy: display !== input,
  };
}

export { hasDesktopLocaleBridge, hasWindowsAnalysisBridge } from "./bridge";
export type { CallEnvelope, ResultEnvelope } from "./generated/contracts";
export function createIpcClient(transport: Transport = invokeNative) {
  return {
    call(envelope: CallEnvelope) {
      return transport("du_call", { envelope });
    },
  };
}
