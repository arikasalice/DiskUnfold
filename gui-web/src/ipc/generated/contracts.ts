export type HistoryEndState = "done" | "partial" | "failed" | "cancelled" | "interrupted";
export type HistoryRelation = "retry" | "restore" | "move_undo" | "effect_check";
export type HistoryListFilter = { finishedFromMs: number | null, finishedBeforeMs: number | null, kinds: Array<string>, states: Array<HistoryEndState>, relations: Array<HistoryRelation>, };
export type HistoryListRequest = { filter: HistoryListFilter | null, cursor: string | null, count: number | null, };
export type HistoryListRevision = { deleteGeneration: string, maxRecordSeq: string, };
export type HistoryChangedBody = {
/**
 * The committed revision, encoded in the same form as a list page.
 */
revision: HistoryListRevision,
/**
 * Open reason key for consumers that may lag behind the producer.
 */
reason: string,
/**
 * The number of affected records as a decimal string.
 */
count: string,
/**
 * The UTC time observed for the committed change.
 */
observedAt: Timestamp, };
export type HistoryListCounts = { completed: string, overwritten: string, failed: string, skipped: string, unprocessed: string, };
export type HistoryListSummary = { kindId: string, kindVersion: number, scopeSummary: string | null, state: HistoryEndState, counts: HistoryListCounts | null, startedAtMs: number | null, finishedAtMs: number | null, predecessorJobRef: Id | null, relation: HistoryRelation | null, sessionRestoreAvailable: boolean | null, };
export type HistoryListRow = { jobRef: Id | null, summary: HistoryListSummary | null,
/**
 * Empty when complete. Each entry is a published omission reason.
 */
missingReasons: Array<string>, };
export type HistoryListResponse = { revision: HistoryListRevision, rows: Array<HistoryListRow>, nextCursor: string | null, };
export type HistoryDetailRequest = { jobRef: Id, };
export type HistoryWorkflow = {
/**
 * An empty list means the operation has no plan lineage.
 */
planVersions: Array<Id> | null, executedPlan: Id | null, approvalCheck: string | null, };
export type HistoryFollowUp = { jobRef: Id | null, relation: HistoryRelation | null, state: HistoryEndState | null, finishedAtMs: number | null, };
export type HistoryEffectValue = {
/**
 * Decimal text preserves precision and sign across IPC.
 */
value: string, unit: string, uncertainty: string, };
export type HistoryEffects = { estimated: HistoryEffectValue | null, executed: HistoryEffectValue | null, confirmed: HistoryEffectValue | null, };
export type HistoryDetailCompleteness = "complete" | "partial";
export type HistoryDetail = { summary: HistoryListSummary | null, workflow: HistoryWorkflow, followUps: Array<HistoryFollowUp> | null, effects: HistoryEffects, completeness: HistoryDetailCompleteness, missingReasons: Array<string>, };
export type HistoryItemOutcome = "completed" | "overwritten" | "skipped" | "failed" | "unprocessed";
export type HistoryItemsRequest = { jobRef: Id, outcome: HistoryItemOutcome | null, cursor: string | null, count: number | null, };
export type HistoryDisplacedItem = { name: string | null, originalLocationDisplay: string | null, trashedAtMs: number | null,
/**
 * A different row of the same history record has the same displayed identity.
 */
ambiguous: boolean, };
export type HistoryItemResult = { locationDisplay: string, outcome: HistoryItemOutcome, reasonKey: string | null, reviewNeeded: boolean, held: boolean, outcomeUnknown: boolean, observedSize: ByteSize | null,
/**
 * Persisted evidence key: exact, lower_bound, or unknown.
 */
observedSizeState: string, displaced: HistoryDisplacedItem | null, };
export type HistoryItemRow = { itemSeq: string, result: HistoryItemResult | null, };
export type HistoryItemCounts = {
/**
 * None when the operation has no readable item summary.
 */
summary: string | null, recorded: string,
/**
 * Difference between summary and recorded counts, if both are known.
 */
unavailable: string | null, };
export type HistoryItemsResponse = { revision: HistoryListRevision, rows: Array<HistoryItemRow>, counts: HistoryItemCounts, completeness: HistoryDetailCompleteness, missingReasons: Array<string>, nextCursor: string | null, };
export type Id = string;
export type ByteSize = string;
export type Timestamp = string;
export type SizeBasis = "logical" | "allocated" | "unique_physical";
export type SizeEvidence = "measured" | "os_reported" | "estimated" | "unknown";
export type SizeAmount = { "state": "exact", bytes: ByteSize, } | { "state": "lower_bound", bytes: ByteSize, } | { "state": "unknown" };
export type SizeValue = { basis: SizeBasis, evidence: SizeEvidence, } & ({ "state": "exact", bytes: ByteSize, } | { "state": "lower_bound", bytes: ByteSize, } | { "state": "unknown" });
export type VizDecimal = string;
export type VizAmount = { "state": "exact", value: VizDecimal, } | { "state": "lower_bound", value: VizDecimal, } | { "state": "unknown", reasonKey: string, };
export type VizQuantity = { unit: string, } & ({ "state": "exact", value: VizDecimal, } | { "state": "lower_bound", value: VizDecimal, } | { "state": "unknown", reasonKey: string, });
export type VizFallback = { modelRef: string, resultRevision: Id, };
export type VizColorMode = "parent_folder" | "classification" | "monochrome";
export type VizLabelMode = "name_size" | "name_only";
export type VizPoint = { x: number, y: number, };
export type VizGeometry = { primitive: string, points: Array<VizPoint>, };
export type ShapeMeta = {
/**
 * Opaque, unique identifier scoped to this model.
 */
shapeRef: string,
/**
 * Closest returned ancestor, or null when none was returned.
 */
parentShapeRef: string | null,
/**
 * Depth from the navigation root, including any omitted ancestors.
 */
visibleDepth: number,
/**
 * First folder in the branch, from the same result revision.
 */
rootBranchRef: ItemRef | null, };
export type VizRelationship = {
/**
 * Registered key preserved even when the consumer has no matching style.
 */
kind: string, fromShapeRef: string, toShapeRef: string,
/**
 * Drawing information, when the strategy supplies geometry for the relationship.
 */
geometry?: VizGeometry, };
export type VizDenominatorBasis = "parent_folder" | "scope" | "analysis_total";
export type VizRatio = string;
export type VizDenominator = { basis: VizDenominatorBasis, value: VizQuantity, size?: SizeValue, };
export type VizRow = { itemRef: ItemRef | null, groupRef: string | null, displayName: string, size: SizeValue, quantity?: VizQuantity, ratio: VizRatio | null, denominator: VizDenominator, legendClassKey: string, displayFlags: Array<string>, };
export type VizRowsRequest = { modelRef: string, resultRevision: Id, start: number, count: number, };
export type VizRows = { rows: Array<VizRow>, totalCount: number, resultRevision: Id, observedAt: Timestamp | null, completeness: ResultCompleteness, };
export type PathEncoding = "unix" | "windows_utf16le";
export type PathBytes = string;
export type OsPath = { encoding: PathEncoding, raw: PathBytes, display: string, lossy: boolean, };
export type ItemRef = { resultVersion: Id, itemNo: number, };
export type ErrorCode = "AUTH_REQUIRED" | "ORIGIN_DENIED" | "SCOPE_DENIED" | "INVALID_ARGUMENT" | "NOT_FOUND" | "STALE_VERSION" | "BUSY" | "INTERNAL";
export type ErrorEnvelope = { code: ErrorCode, reasonKey: string, messageKey: string, correlationId: Id, itemRefs?: Array<ItemRef>, retryable: boolean, };
export type CallEnvelope = { name: string, contractVersion: number, correlationId: Id, body: unknown, };
export type ResultEnvelope = { name: string, contractVersion: number, correlationId: Id, body: unknown, error?: ErrorEnvelope, };
export type EventEnvelope = { topic: string, contractVersion: number, seq: number, correlationId?: Id, jobRef?: Id, resultVersion?: Id, occurredAt: Timestamp, body: unknown, };
export type ResultVersionChange = "published" | "withdrawn";
export type PublishedCompleteness = "complete" | "partial" | "unknown";
export type ResultVersionChanged = { lineageId: Id, sourceRef: Id, origin: string, sequence: number, change: ResultVersionChange, previousResultVersion?: Id, completeness?: PublishedCompleteness, observedAt?: Timestamp, };
export type TargetKind = "internal" | "external" | "network_mount" | "folder_mount";
export type IdentitySource = "confirmed" | "estimated" | "unavailable";
export type TargetAccess = "checking" | "accessible" | "denied" | "disconnected" | "missing" | "unresponsive" | "unknown";
export type TargetListRequest = { refresh: boolean, };
export type AnalysisTarget = { kind: TargetKind | null, path: OsPath, connected: boolean, volumeId: string | null, identitySource: IdentitySource, access: TargetAccess, reasonKey: string | null, };
export type TargetList = { targets: Array<AnalysisTarget>, probeJobRef: Id | null, observedAt: Timestamp, unavailableReason: string | null, };
export type TargetStatusChanged = { target: OsPath, access: TargetAccess, reasonKey: string | null, totalBytes: ByteSize | null, availableBytes: ByteSize | null, capacityReason: string | null, aggregationUncertain: boolean, observedAt: Timestamp, };
export type TargetProbeEvent = { sequence: number, target: OsPath, access: TargetAccess, reasonKey: string | null, totalBytes: ByteSize | null, availableBytes: ByteSize | null, capacityReason: string | null, aggregationUncertain: boolean, observedAt: Timestamp, };
export type ScopeSource = "gui" | "recent" | "catalog" | "cli" | "mcp";
export type ScopeBoundary = "current_volume" | "include_selected";
export type ScopeExclusion = { "kind": "path", path: OsPath, } | { "kind": "name_pattern", pattern: string, };
export type ScopeDraft = { includes: Array<OsPath>, exclusions: Array<ScopeExclusion>, boundary: ScopeBoundary, includedMounts: Array<OsPath>, source: ScopeSource, };
export type ScopeReviewRequest = { draft: ScopeDraft, requestSeq: number, };
export type ScopeCorrespondence = { path: OsPath, access: TargetAccess, reasonKey: string | null, };
export type ScopeReview = { scope: ScopeDraft, notices: Array<string>, targets: Array<ScopeCorrespondence>, reviewFingerprint: string, observedAt: Timestamp, requestSeq: number, };
export type ScanStartRequest = { draft: ScopeDraft, reviewFingerprint: string, };
export type ScanStart = { jobRef: Id, existing: boolean, scopeVersion: Id, scopeFingerprint: string, lineageId?: Id, };
export type ScanFailureKind = "collection_failed" | "unread_range" | "uncertain" | "untraversed_link";
export type ScanFailureStage = "metadata" | "open_directory" | "enumerate" | "traverse";
export type ScanFailurePathRef = { rootIndex: number, relative: Array<PathBytes>, };
export type ScanFailureRow = { kind: ScanFailureKind, stage: ScanFailureStage, pathRef: ScanFailurePathRef, reasonKey: string, observedAt?: Timestamp, displayPath?: string, };
export type ScanFailuresRequest = { resultVersion: Id, kind?: ScanFailureKind, cursor?: string, count?: number, };
export type ScanFailures = { resultVersion: Id, rows: Array<ScanFailureRow>, totalCount: number, nextCursor?: string, };
export type JobState = "queued" | "running" | "pause_requested" | "paused" | "cancel_requested" | "done" | "partial" | "failed" | "cancelled";
export type AnalysisJobRequest = { jobRef: Id, };
export type ScanResult = { lineageId: Id, sequence: number, resultVersion?: Id, };
export type AnalysisJobStatus = { jobRef: Id, kind: string, state: JobState, sequence: number, resultVersion: Id | null,
/**
 * Published query result for a successfully completed query evaluation.
 */
queryRef?: Id, reasonKey: string | null, scanResult?: ScanResult, };
export type JobControlOutcome = "accepted" | "no_effect" | "unsupported" | "already_terminal";
export type AnalysisJobControl = { outcome: JobControlOutcome, state: JobState, reasonKey: string | null, sequence: number, };
export type SpaceTarget = { "kind": "scope_root", } | { "kind": "folder", item: ItemRef, } | { "kind": "query_result", queryRef: Id, };
export type SpaceTotalRequest = { resultVersion?: Id, target: SpaceTarget, requestSeq: number, };
export type ResultCompleteness = "partial" | "complete" | "missing" | "cancelled";
export type SpaceTotalSize = { size: SizeValue, completeness: ResultCompleteness, measurementLimits: Array<string>, };
export type SpaceTotal = { resultVersion: Id, requestSeq: number, sizes: Array<SpaceTotalSize>, completeness: ResultCompleteness, missingItems: ByteSize | null, inaccessibleRanges: ByteSize | null, untraversedLinks: ByteSize | null, };
export type VolumeGapSelection = { volumeId?: PathBytes | null, root?: ItemRef | null, };
export type VolumeGapRequest = { resultVersion: Id, requestSeq: number, target: VolumeGapSelection, };
export type VolumeGapObservation = { "state": "not_attempted" } | { "state": "unavailable", observedAt: Timestamp | null, reasonKey: string, } | { "state": "available", observedAt: Timestamp | null, totalBytes: ByteSize, usedBytes: ByteSize, freeBytes: ByteSize, evidence: SizeEvidence, };
export type VolumeGapSize = { size: SizeValue, completeness: ResultCompleteness, };
export type VolumeGapAmount = { "state": "exact", bytes: ByteSize, evidence: SizeEvidence, } | { "state": "unknown", reasonKey: string, };
export type VolumeGapRow = { key: string, value: VolumeGapAmount, };
export type VolumeGapCategories = { "state": "unavailable", reasonKey: string, } | { "state": "available", rows: Array<VolumeGapRow>, };
export type VolumeGapResponse = { resultVersion: Id, requestSeq: number, volumeId: PathBytes | null, start: VolumeGapObservation, end: VolumeGapObservation, allocated: VolumeGapSize, uniquePhysical: VolumeGapSize, factors: Array<VolumeGapRow>, remainder: VolumeGapAmount, osCategories: VolumeGapCategories, };
export type ResultItemKind = "directory" | "file" | "untraversed_link" | "unknown" | "unknown_structure";
export type BrowseValue<T> = { "state": "present", value: T, } | { "state": "absent" } | { "state": "unknown", reasonKey: string, };
export type BrowseOrigin = "scan";
export type BrowseSourceKind = "scan" | "snapshot" | "catalog" | "query";
export type BrowseSourceRef = { kind: BrowseSourceKind, ref: Id, };
export type BrowseViewKind = "list" | "tree_branch";
export type BrowseSortKey = { id: string, revision: number, descending: boolean, sizeBasis?: SizeBasis | null, };
export type QueryNameOp = "equal" | "contains" | "starts_with" | "ends_with";
export type QueryNameEncoding = "bytes" | "windows_utf16le";
export type QueryItemType = "file" | "directory" | "link";
export type QuerySizeBasis = "logical" | "allocated";
export type QuerySizeOp = "at_least" | "greater_than" | "at_most" | "less_than";
export type QueryPredicate = { "kind": "name", op: QueryNameOp, encoding: QueryNameEncoding, value: PathBytes, } | { "kind": "type", value: QueryItemType, } | { "kind": "size", basis: QuerySizeBasis, op: QuerySizeOp, bytes: ByteSize, } | { "kind": "registered", id: string, revision: number, operand: PathBytes, };
export type QueryCondition = { "kind": "all", children: Array<QueryCondition>, } | { "kind": "any", children: Array<QueryCondition>, } | { "kind": "not", child: QueryCondition, } | { "kind": "predicate", predicate: QueryPredicate, };
export type BrowseOpenRequest = { regionRef: Id, viewKind: BrowseViewKind, requestSeq: number, sourceRef?: BrowseSourceRef | null, resultVersion?: Id | null, location?: ItemRef | null, sortKeys?: Array<BrowseSortKey> | null, condition?: QueryCondition | null, pageSize?: number | null, };
export type ResultOrigin = string;
export type QueryStrategy = { id: string, revision: number, };
export type IdentityCapability = { state: SpaceCapabilityState, reasonKey?: string | null, };
export type CorrespondenceKey = { volumeId: PathBytes, pathCompareKey: PathBytes, fileId: PathBytes | null, identityCapability: IdentityCapability, };
export type FolderFlags = { unenumerated: BrowseValue<boolean>, excluded: BrowseValue<boolean>, };
export type FolderQueryRow = { item: ItemRef, displayName: BrowseValue<string>, kind: ResultItemKind, logicalSize: BrowseValue<SizeValue>, allocatedSize: BrowseValue<SizeValue>, modifiedAt: BrowseValue<Timestamp>, hasChildFolder: BrowseValue<boolean>, flags: FolderFlags, correspondence: BrowseValue<CorrespondenceKey>, };
export type SearchQueryRow = { item: ItemRef, displayName: BrowseValue<string>, displayPath: BrowseValue<string>, kind: ResultItemKind, logicalSize: BrowseValue<SizeValue>, allocatedSize: BrowseValue<SizeValue>, modifiedAt: BrowseValue<Timestamp>, createdAt: BrowseValue<Timestamp>, missingFields: Array<string>, reasonKeys: Array<string>, };
export type QueryTotal = { "state": "exact", count: number, } | { "state": "lower_bound", count: number, };
export type FolderRowKind = "folder";
export type SearchRowKind = "search";
export type QueryPage<R> = { queryRef: Id, resultVersion: Id, rows: Array<R>, total: QueryTotal, unknownCount: number, observedAt: Timestamp | null, completeness: ResultCompleteness, nextCursor: string | null, };
export type FolderQueryPage = { rowKind: FolderRowKind,
/**
 * Location context for a directly requested first page, when available.
 */
browseContext?: BrowseContext, queryRef: Id, resultVersion: Id, rows: Array<FolderQueryRow>, total: QueryTotal, unknownCount: number, observedAt: Timestamp | null, completeness: ResultCompleteness, nextCursor: string | null, };
export type SearchQueryPage = { rowKind: SearchRowKind, queryRef: Id, resultVersion: Id, rows: Array<SearchQueryRow>, total: QueryTotal, unknownCount: number, observedAt: Timestamp | null, completeness: ResultCompleteness, nextCursor: string | null, };
export type QueryPageResponse = FolderQueryPage | SearchQueryPage;
export type BrowseBreadcrumb = { item: ItemRef, displayName: BrowseValue<string>, };
export type BrowseRootKind = "virtual_root" | "scope_root" | "folder";
export type BrowseContext = { origin: ResultOrigin, location: ItemRef | null, breadcrumbs: Array<BrowseBreadcrumb>, rootKind: BrowseRootKind, currentCorrespondence: boolean, sortStrategies: Array<QueryStrategy>, predicateStrategies: Array<QueryStrategy>, };
export type BrowseOpenResponse = { "kind": "opened", requestSeq: number, origin: ResultOrigin, location: ItemRef | null, breadcrumbs: Array<BrowseBreadcrumb>, rootKind: BrowseRootKind, currentCorrespondence: boolean, page: FolderQueryPage, sortStrategies: Array<QueryStrategy>, predicateStrategies: Array<QueryStrategy>, } | { "kind": "job", requestSeq: number, jobRef: Id,
/**
 * Source result version pinned when the request was admitted.
 */
resultVersion: Id, };
export type BrowseDetailRequest = { resultVersion: Id, item: ItemRef, };
export type BrowseDetail = { item: ItemRef, displayPath: BrowseValue<string>, displayName: BrowseValue<string>, kind: BrowseValue<ResultItemKind>, logicalSize: BrowseValue<SizeValue>, allocatedSize: BrowseValue<SizeValue>, modifiedAt: BrowseValue<Timestamp>, createdAt: BrowseValue<Timestamp>, observedAt: BrowseValue<Timestamp>, origin: BrowseOrigin, completeness: ResultCompleteness, currentCorrespondence: boolean, observationOnly: boolean, };
export type QueryPagePosition = { queryRef: Id, resultVersion: Id, start: number, count: number, };
export type QueryPageContinuation = { nextCursor: string, count?: number | null, };
export type QueryPageRequest = QueryPagePosition | QueryPageContinuation;
export type QueryRegionKind = "search" | "browse_list" | "browse_tree_branch" | "workspace_classification";
export type QueryRegionOpenRequest = { kind: QueryRegionKind, };
export type QueryRegionOpened = { regionRef: Id, };
export type QueryRegionCloseRequest = { regionRef: Id, };
export type QueryRegionClosed = Record<string, never>;
export type QueryCancelTarget = { "kind": "request", requestSeq: number, } | { "kind": "job", jobRef: Id, };
export type QueryCancelRequest = { regionRef: Id, target: QueryCancelTarget, };
export type QueryCancelOutcome = "accepted" | "no_effect" | "already_terminal";
export type QueryCancelled = { outcome: QueryCancelOutcome, };
export type SpaceDetailRequest = { resultVersion: Id, item: ItemRef, };
export type SpaceDetailSize = { size: SizeValue, measurementLimits: Array<string>, };
export type SpaceLinkGroup = {
/**
 * Total OS-reported links, serialized as an exact decimal count.
 */
reportedLinkCount: string | null, inScopePaths: Array<ItemRef>, hasOutsideLinks: boolean | null, };
export type SpaceExclusiveDecision = { confirmed: boolean, reasonKey: string | null, };
export type SpacePlaceholderState = "none" | "partial" | "full";
export type SpaceAuxiliary = { size: SizeValue, includedInAllocated: boolean | null, };
export type SpaceCapabilityState = "supported" | "partial" | "unsupported";
export type SpaceDetailCapability = { key: string, state: SpaceCapabilityState, reasonKey: string | null, };
export type SpaceDetail = { item: ItemRef, logical: BrowseValue<SpaceDetailSize>, allocated: BrowseValue<SpaceDetailSize>, uniquePhysical: BrowseValue<SpaceDetailSize>, exclusive: BrowseValue<SpaceDetailSize>, shared: BrowseValue<SpaceDetailSize>, linkGroup: BrowseValue<SpaceLinkGroup>, placeholderLocalState?: BrowseValue<SpacePlaceholderState>, auxiliary: BrowseValue<SpaceAuxiliary>, exclusiveDecision: SpaceExclusiveDecision, observedAt: BrowseValue<Timestamp>, capabilities: BrowseValue<Array<SpaceDetailCapability>>, };
export const SIZE_UNIT_RULES = {"decimalPlaces":1,"iec":{"base":1024,"symbols":["B","KiB","MiB","GiB","TiB","PiB"]},"revision":1,"si":{"base":1000,"symbols":["B","kB","MB","GB","TB","PB"]}} as const;
