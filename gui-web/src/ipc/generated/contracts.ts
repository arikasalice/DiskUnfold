export type DisplayPreferencesRequest = Record<symbol, never>;
export type DisplayUnitSystem = "si" | "iec";
export type DisplayPreferences = {
/**
 * Canonical decimal representation of the settings revision.
 */
settingsRevision: string,
/**
 * Selected unit system.
 */
unitSystem: DisplayUnitSystem,
/**
 * Automatic selection or a symbol supported by the selected system.
 */
unit: string,
/**
 * Default size basis for new views.
 */
defaultBasis: SizeBasis,
/**
 * Settings restoration or unit-pair warning, if present.
 */
warningKey: string | null, };
export type SettingsRestoreRequest = Record<symbol, never>;
export type SettingsRestoreSource = "restored" | "default";
export type SettingsRestoreItem = {
/**
 * Stable key of the registered setting.
 */
key: string,
/**
 * Source selected when restoring this setting.
 */
source: SettingsRestoreSource,
/**
 * Reason for falling back to a default, if a failure occurred.
 */
reasonKey: string | null, };
export type SettingsRestoreStatus = {
/**
 * Restoration outcome for each registered setting.
 */
items: Array<SettingsRestoreItem>,
/**
 * Absent if the stored schema could not be read.
 */
storedSchema: number | null,
/**
 * Settings schema supported by this application.
 */
appSchema: number,
/**
 * Whether startup successfully migrated the stored settings.
 */
migrated: boolean,
/**
 * Whether the startup outcome disallows normal settings writes.
 */
writeLocked: boolean, };
export type StartupStatusRequest = Record<symbol, never>;
export type StartupCheck = string;
export type StartupCheckOutcome = string;
export type StartupGrade = string;
export type StartupCheckResult = {
/**
 * The check represented by this row.
 */
check: StartupCheck,
/**
 * The observed outcome. Unknown values remain safe to display generically.
 */
result: StartupCheckOutcome,
/**
 * An open reason key when the check needs to explain its outcome.
 */
reasonKey: string | null, };
export type StartupAccessState = string;
export type StartupStatus = {
/**
 * The most severe applicable advisory result.
 */
grade: StartupGrade,
/**
 * Results in the published startup check order.
 */
checks: Array<StartupCheckResult>,
/**
 * Whether the application created the user's startup database.
 */
firstRun: boolean,
/**
 * Full-disk-access state observed for the current startup.
 */
access: StartupAccessState,
/**
 * Whether analysis entry is allowed in the current session.
 */
analysisAllowed: boolean,
/**
 * Where the running application is installed, when the platform can tell.
 *
 * Omitted when unknown. Full Disk Access cannot be granted usefully to an app that
 * runs from `elsewhere`, so the screen asks the user to move it first.
 */
appLocation?: StartupAppLocation, };
export type StartupAccessAction = "open_privacy_settings" | "recheck";
export type StartupAppLocation = "applications" | "elsewhere";
export type StartupAccessRequest = {
/**
 * Action to perform.
 */
action: StartupAccessAction, };
export type StartupAccessResponse = {
/**
 * Observed full-disk-access state; unknown is never treated as denied.
 */
access: StartupAccessState,
/**
 * Whether analysis entry is allowed under the observed access state.
 */
analysisAllowed: boolean,
/**
 * Whether the operating system privacy settings screen was opened.
 */
settingsOpened: boolean, };
export type InstallForm = "msi" | "portable" | "msix" | "dmg" | "deb" | "tar_gz" | "unknown";
export type NoticeKind = "LICENSE" | "THIRD-PARTY-NOTICES";
export type ProductInfoRequest = Record<symbol, never>;
export type SupportInfoRequest = Record<symbol, never>;
export type NoticeAvailability = {
/**
 * Document kind.
 */
kind: NoticeKind,
/**
 * Whether the executable embeds a body for this document.
 */
available: boolean, };
export type ProductInfo = {
/**
 * Product version string.
 */
productVersion: string | null,
/**
 * Official or developer build marker; a developer build is never verified output.
 */
buildKind: BuildKind,
/**
 * Source commit recorded at build time.
 */
commit: string | null,
/**
 * Build time in UTC (RFC 3339).
 */
builtAtUtc: string | null,
/**
 * Target operating system recorded at build time.
 */
targetOs: string | null,
/**
 * Target CPU architecture recorded at build time.
 */
targetCpu: string | null,
/**
 * Observed install form; unknown is never promoted to a guess.
 */
installForm: InstallForm,
/**
 * `product.install_form_unknown` when the install form was not established.
 */
installFormReasonKey: string | null,
/**
 * Application database format version.
 */
appDbFormat: number | null,
/**
 * Snapshot file format version.
 */
snapshotFormat: number | null,
/**
 * Embedded support-matrix revision.
 */
supportMatrixRevision: string | null,
/**
 * Embedded support-matrix preparation date (YYYY-MM-DD).
 */
supportMatrixAsOf: string | null,
/**
 * Notice documents with their availability, in a fixed order.
 */
notices: Array<NoticeAvailability>, };
export type NoticesRequest = {
/**
 * Document kind to read.
 */
kind: NoticeKind, };
export type NoticesResponse = {
/**
 * Requested document kind.
 */
kind: NoticeKind,
/**
 * Document body, or `None` when this build embeds no such document.
 */
text: string | null,
/**
 * Commit from which the embedded notices were generated, when recorded.
 */
sourceCommit: string | null, };
export type ExecutableDigestRequest = {
/**
 * Correlation identifier the caller uses to match the response.
 */
correlationId: Id, };
export type ExecutableDigestResponse = {
/**
 * Echo of the request correlation identifier.
 */
correlationId: Id,
/**
 * Lowercase hexadecimal SHA-256 of the running executable, if it could be read.
 */
sha256: string | null,
/**
 * `product.digest_unavailable` when the digest could not be computed.
 */
reasonKey: string | null, };
export type BuildKind = "official" | "developer" | "unknown";
export type SupportOsFamily = "windows" | "macos" | "linux" | "unknown";
export type SupportCpu = "x64" | "arm64" | "other" | "unknown";
export type SupportStatus = "officially_verified" | "conditional" | "unsupported_found" | "not_verified" | "unknown";
export type SupportCapabilityState = "supported" | "limited" | "unsupported" | "not_applicable" | "unknown";
export type SecuritySupportKind = "general" | "extended" | "unknown";
export type SupportEnvironmentIdentity = {
/**
 * Operating-system family. Unknown is never used as a wildcard.
 */
osFamily: SupportOsFamily,
/**
 * Operating-system release or distribution release identifier.
 */
release: string | null,
/**
 * Edition or distribution identifier.
 */
edition: string | null,
/**
 * CPU architecture of the running process.
 */
cpu: SupportCpu, };
export type SecuritySupportBasis = {
/**
 * Source reference supporting the vendor's security-support status.
 */
source: string | null,
/**
 * Date the source was checked, formatted as YYYY-MM-DD.
 */
checkedOn: string | null,
/**
 * Whether the vendor reports general or extended support.
 */
kind: SecuritySupportKind | null, };
export type SupportCombinationIdentity = {
/**
 * Operating-system family for this row.
 */
osFamily: SupportOsFamily,
/**
 * Release or distribution release identifier.
 */
release: string | null,
/**
 * Edition or distribution identifier.
 */
edition: string | null,
/**
 * CPU architecture for this row.
 */
cpu: SupportCpu, };
export type SupportFeatureRow = {
/**
 * Stable feature-area key.
 */
area: string,
/**
 * Support-table status for this feature area.
 */
status: SupportStatus,
/**
 * Recorded conditions or restrictions, preserved as source text.
 */
conditions: Array<string>, };
export type SupportCombination = {
/**
 * Exact identity of this combination.
 */
identity: SupportCombinationIdentity,
/**
 * Status derived from native test records for this exact combination.
 */
status: SupportStatus,
/**
 * Vendor security-support source and check date, when recorded.
 */
securitySupport: SecuritySupportBasis | null,
/**
 * Detailed configuration and execution-form identifiers covered by tests.
 */
verifiedDetails: Array<string>,
/**
 * References to the native test records used to derive this row's status.
 */
verificationRecords: Array<string>,
/**
 * Feature-area support rows for this exact combination.
 */
features: Array<SupportFeatureRow>, };
export type SupportFeatureAssessment = {
/**
 * Stable feature-area key.
 */
area: string,
/**
 * Status recorded in the matching support-table row.
 */
supportTableStatus: SupportStatus,
/**
 * Current capability value reported by the platform adapter.
 */
currentCapability: SupportCapabilityState,
/**
 * Recorded support conditions for the feature area.
 */
conditions: Array<string>,
/**
 * Stable reason key for an unknown or limited current capability.
 */
reasonKey: string | null, };
export type SupportReport = {
/**
 * Embedded support-matrix revision, if present.
 */
matrixRevision: string | null,
/**
 * Date the embedded matrix was prepared.
 */
asOf: string | null,
/**
 * Current process environment identity.
 */
environment: SupportEnvironmentIdentity,
/**
 * Status of the exact current OS and CPU combination.
 */
combinationStatus: SupportStatus,
/**
 * Security-support source and date, separate from combination status.
 */
securitySupport: SecuritySupportBasis | null,
/**
 * Feature-area support and current capability rows.
 */
features: Array<SupportFeatureAssessment>,
/**
 * All rows from the embedded matrix, in their published order.
 */
combinations: Array<SupportCombination>,
/**
 * Stable reason key for an unknown or unverified combination.
 */
reasonKey: string | null, };
export type AppQuitMode = "normal" | "without_waiting";
export type AppQuitRequest = {
/**
 * Shutdown mode selected by the GUI; its two wire tokens are inferred.
 */
mode: AppQuitMode,
/**
 * Whether the user confirmed not waiting; omission means false and the
 * wire property spelling is inferred.
 */
confirmUnfinishedChanges?: boolean, };
export type AppQuitOutcome = string;
export type AppQuitResponse = {
/**
 * Accepted, already-closing, or confirmation-required outcome; the
 * property spelling is inferred.
 */
outcome: AppQuitOutcome,
/**
 * In-progress change-work count for confirmation; the property spelling
 * is inferred.
 */
inProgressChangeCount?: number, };
export type AppLifecycleState = string;
export type AppLifecycleChanged = {
/**
 * Current application lifecycle state; the property spelling is inferred.
 */
state: AppLifecycleState,
/**
 * State immediately before the transition; the property spelling is
 * inferred.
 */
previousState: AppLifecycleState,
/**
 * Number of jobs still unfinished during shutdown; the property spelling
 * is inferred.
 */
remainingJobCount: number,
/**
 * Monotonically increasing state sequence for this app session; the
 * property spelling is inferred.
 */
stateSequence: number,
/**
 * UTC observation time in the shared timestamp format; the property
 * spelling is inferred.
 */
observedAt: Timestamp,
/**
 * Present and true when the window asked to close while change work is in
 * progress and the host is waiting for the user's confirmation. The state
 * stays `running`; `remainingJobCount` keeps meaning every remaining job.
 *
 * This optional field is an addition to the published IF-07 body. Readers
 * that do not know it ignore it. See the F15 design deviation notes.
 */
confirmationRequired?: boolean,
/**
 * In-progress change jobs the confirmation concerns; present with
 * `confirmationRequired`. An optional addition to the published body.
 */
inProgressChangeCount?: number, };
export type SettingsApplyTiming = "immediate" | "next_query" | "next_job" | "next_start" | "owner_review";
export type SettingsChangeSource = "screen" | "import" | "reset" | "owner" | "mcp";
export type SettingsChangedKey = {
/**
 * Registered settings key.
 */
key: string,
/**
 * Application timing assigned by the owner.
 */
applyTiming: SettingsApplyTiming, };
export type SettingsChangedBody = {
/**
 * Previous settings revision as a canonical unsigned decimal string.
 */
previousSettingsRevision: string,
/**
 * Committed settings revision as a canonical unsigned decimal string.
 */
settingsRevision: string,
/**
 * The distinct changed keys and their application timing.
 */
changes: Array<SettingsChangedKey>,
/**
 * Source of this commit.
 */
source: SettingsChangeSource, };
export type LocaleProfileRevision = string;
export type LocaleLanguageSource = "stored" | "os" | "baseline";
export type LocaleDateFormat = "lang_default" | "iso8601";
export type LocaleNumberFormat = "lang_default" | "no_grouping";
export type LocaleTimeZonePolicy = "local" | "utc";
export type LocalTimeZoneStatus = "identified" | "offset_only" | "unavailable";
export type LocaleMotion = "follow_os" | "reduce";
export type LocaleAppScale = 100 | 110 | 125 | 150 | 175 | 200;
export type LocaleSupportedLanguage = {
/**
 * Registered language identifier.
 */
language: string,
/**
 * Message key for the language's self-name.
 */
selfNameKey: string,
/**
 * Opaque revision of this language's bundle.
 */
bundleRevision: string, };
export type LocaleProfile = {
/**
 * Selected registered language identifier.
 */
displayLanguage: string,
/**
 * Source that selected the startup display language.
 */
languageSource: LocaleLanguageSource,
/**
 * Selected date format.
 */
dateFormat: LocaleDateFormat,
/**
 * Selected number format.
 */
numberFormat: LocaleNumberFormat,
/**
 * Selected timestamp time zone policy.
 */
timeZonePolicy: LocaleTimeZonePolicy,
/**
 * Observed status of the local time zone.
 */
localTimeZoneStatus: LocalTimeZoneStatus,
/**
 * Selected application motion preference.
 */
motion: LocaleMotion,
/**
 * Selected application display scale in percent.
 */
appScale: LocaleAppScale,
/**
 * Canonical decimal profile snapshot revision.
 */
profileRevision: LocaleProfileRevision,
/**
 * Supported languages ordered by language identifier.
 */
supportedLanguages: Array<LocaleSupportedLanguage>, };
export type LocaleProfileChanges = {
/**
 * New registered language identifier, when supplied.
 */
displayLanguage?: string,
/**
 * New date format, when supplied.
 */
dateFormat?: LocaleDateFormat,
/**
 * New number format, when supplied.
 */
numberFormat?: LocaleNumberFormat,
/**
 * New time zone policy, when supplied.
 */
timeZonePolicy?: LocaleTimeZonePolicy,
/**
 * New application motion preference, when supplied.
 */
motion?: LocaleMotion,
/**
 * New application display scale in percent, when supplied.
 */
appScale?: LocaleAppScale, };
export type LocaleGetProfileRequest = Record<string, never>;
export type LocaleProfileSetRequest = {
/**
 * Current profile revision required for this conditional update.
 */
expectedProfileRevision: LocaleProfileRevision,
/**
 * Nonempty fields to update. Omitted fields are retained.
 */
changes: LocaleProfileChanges, };
export type LocaleChangedField = "display_language" | "date_format" | "number_format" | "time_zone_policy" | "motion" | "app_scale";
export type LocaleChangedBody = {
/**
 * New canonical decimal profile snapshot revision.
 */
profileRevision: LocaleProfileRevision,
/**
 * Changed fields in declaration order, without duplicates.
 */
changedFields: Array<LocaleChangedField>,
/**
 * New selected registered language identifier.
 */
displayLanguage: string, };
export type MessageParameterKind = "count" | "size" | "timestamp" | "number" | "path" | "name" | "message_key";
export type MessageTemplate = { "kind": "text",
/**
 * Named parameters and their types.
 */
parameters: { [key in string]: MessageParameterKind },
/**
 * Template text using the registered template syntax.
 */
template: string, } | { "kind": "plural",
/**
 * Named parameters and their types.
 */
parameters: { [key in string]: MessageParameterKind },
/**
 * Parameter whose value selects a variant.
 */
parameter: string,
/**
 * Plural category to template text.
 */
variants: { [key in string]: string }, };
export type LocaleDatePattern = "lang_default" | "iso8601";
export type LocaleFormatTable = {
/**
 * Decimal separator symbol.
 */
decimalSeparator: string,
/**
 * Digit grouping separator symbol.
 */
groupingSeparator: string,
/**
 * Positive number of digits in each grouping segment.
 */
groupingSize: number,
/**
 * Date patterns by published format identifier.
 */
datePatterns: { [key in LocaleDatePattern]?: string },
/**
 * Symbols indexed by the published F03 unit identifiers.
 */
unitSymbols: { [key in string]: string }, };
export type ReasonMessageMeta = {
/**
 * Message key describing the cause.
 */
causeKey: string,
/**
 * Message key describing the follow-up action.
 */
actionKey: string,
/**
 * Whether the represented operation cannot be reversed.
 */
irreversible: boolean,
/**
 * Evidence certainty associated with the reason.
 */
certainty: ReasonCertainty,
/**
 * User-visible lifecycle state associated with the reason.
 */
status: ReasonStatus, };
export type ReasonCertainty = "confirmed" | "uncertain" | "unknown" | "not_applicable";
export type ReasonStatus = "normal" | "in_progress" | "complete" | "empty" | "permission_denied" | "disconnected" | "incomplete" | "confirmation" | "cancelled" | "failed";
export type LocaleBundleBody = {
/**
 * Message templates keyed by stable message identifier.
 */
messages: { [key in string]: MessageTemplate },
/**
 * Locale-specific formatting rules.
 */
formatTable: LocaleFormatTable,
/**
 * Registered plural rule identifier.
 */
pluralRule: string,
/**
 * Language used after this bundle when a message is missing.
 */
fallbackLanguage?: string,
/**
 * Owner-defined, language-neutral reason metadata.
 */
reasonMessages: { [key in string]: ReasonMessageMeta },
/**
 * Lowercase BLAKE3 digest of the exact metadata source bytes.
 */
metadataRevision: string, };
export type LocaleBundleRequest = {
/**
 * Registered language identifier requested by the caller.
 */
language: string,
/**
 * The opaque revision for the same-language bundle already held by the caller.
 */
knownBundleRevision?: string, };
export type LocaleBundleResponse = {
/**
 * Current opaque revision for the requested language.
 */
bundleRevision: string,
/**
 * Present unless the request already held this exact revision.
 */
bundle?: LocaleBundleBody, };
export type LocaleMissingMarker = "fallback" | "key";
export type LocaleMissingEntry = {
/**
 * Stable message key that was missing.
 */
key: string,
/**
 * Fallback used or key displayed.
 */
marker: LocaleMissingMarker, };
export type LocaleReportMissingRequest = {
/**
 * Registered language identifier in which the message was missing.
 */
language: string,
/**
 * Keys and missing markers reported in this batch.
 */
entries: Array<LocaleMissingEntry>, };
export type LocaleReportMissingResponse = Record<string, never>;
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
export type VizRenderNonModelResponse = { "kind": "job", requestSeq: number, resultRevision: Id, observedAt: Timestamp | null, completeness: ResultCompleteness, jobRef: Id, } | { "kind": "unavailable", requestSeq: number, resultRevision: Id, observedAt: Timestamp | null, completeness: ResultCompleteness, reasonKey: string, availableFormats: Array<string>, fallback: VizFallback | null, };
export type VizColorMode = "parent_folder" | "classification" | "monochrome";
export type VizLabelMode = "name_size" | "name_only";
export type VizCancelRequest = { target: VizCancelTarget, };
export type VizCancelTarget = { "kind": "subscription", viewChannel: Id, requestSeq: number, } | { "kind": "job", jobRef: Id, };
export type VizControlResult = "accepted" | "no_effect" | "already_terminal";
export type VizCancelResponse = { controlResult: VizControlResult, };
export type VizAvailabilityRequest = { sourceRef: VizSourceRef, resultRevision: Id, aggregateScopeRef: ItemRef, };
export type VizFormatAvailability = { format: string, available: boolean,
/**
 * Null for an available format; required and non-null for an unavailable one.
 */
reasonKey: string | null, };
export type VizAvailabilityResponse = { formats: Array<VizFormatAvailability>, requiredDataTableRevision: number, };
export type VizCatalogRequest = Record<string, never>;
export type VizCatalog = { formats: Array<string>, strategies: Array<QueryStrategy>, measures: Array<string>, colorCriteria: Array<string>, denominatorCriteria: Array<string>, interpretationTableRevision: number, requiredDataTableRevision: number, legendRuleRevision: number, };
export type VizViewOpenRequest = Record<string, never>;
export type VizViewChannel = { viewChannel: Id, };
export type VizSourceRef = { kind: string, ref: Id, };
export type VizCondition = { "kind": "normalized", condition: QueryCondition, } | { "kind": "query_result", queryRef: Id, };
export type VizViewport = { width: number, height: number, scale: number, };
export type VizVisibleWindow = { left: number, top: number, right: number, bottom: number, };
export type VizExpansion = { itemRef: ItemRef, extraDepth: number, };
export type VizRenderRequest = { viewChannel: Id, requestSeq: number, format: string, sourceRef?: VizSourceRef, resultRevision?: Id, aggregateScopeRef: ItemRef, navigationScopeRef: ItemRef, focusItemRef?: ItemRef, condition?: VizCondition, measure: string, sizeBasis: SizeBasis, classificationCriterion: string, colorCriterion: string, denominatorBasis: VizDenominatorBasis, strategy?: QueryStrategy, viewport: VizViewport, visibleWindow: VizVisibleWindow, minAreaPx2: number, depthLimit: number, expandedDepthByItem?: Array<VizExpansion>, colorMode?: VizColorMode, labelMode?: VizLabelMode, };
export type VizStalenessState = "current" | "stale" | "unknown";
export type VizStaleness = { state: VizStalenessState, reasonKey: string | null, };
export type VizRepresentation = { format: string, measure: string, sizeBasis: SizeBasis, classificationCriterion: string, colorCriterion: string, denominatorBasis: VizDenominatorBasis, colorMode: VizColorMode | null, labelMode: VizLabelMode | null, };
export type VizTotal = { quantity: VizQuantity, sourceSize: SizeValue | null, };
export type VizTotals = { scope: VizTotal, drawn: VizTotal, grouped: VizTotal, filteredOut: VizTotal, };
export type VizTableRevisions = { interpretation: number, requiredData: number, legend: number, viewPolicy: number, sizeRule: QueryStrategy, };
export type VizMatchKey = { "state": "present", value: CorrespondenceKey, } | { "state": "unknown", reasonKey: string, } | { "state": "not_applicable", };
export type VizShape = { itemRef: ItemRef | null, groupRef: string | null, matchKey: VizMatchKey, geometry: VizGeometry, size: SizeValue, quantity?: VizQuantity, legendClassKey: string, displayFlags: Array<string>, shapeRef?: string, parentShapeRef?: string | null, visibleDepth?: number, rootBranchRef?: ItemRef | null, };
export type VizLegendEntry = { key: string, labelKey: string, itemCount: VizQuantity, total: VizTotal, condition: QueryCondition | null, };
export type VizSummary = { groupedItems: VizQuantity, omittedItems: VizQuantity, unknownItems: VizQuantity, reasonKeys: Array<string>, };
export type VizModel = { modelRef: string, conditionFingerprint: string, aggregateScopeRef: ItemRef, navigationScopeRef: ItemRef, focusItemRef: ItemRef | null, origin: ResultOrigin,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, staleness: VizStaleness, representation: VizRepresentation, denominator: VizDenominator, totals: VizTotals, shapes: Array<VizShape>, relationships: Array<VizRelationship>, legend: Array<VizLegendEntry>, summary: VizSummary, strategy: QueryStrategy, tableRevisions: VizTableRevisions, };
export type VizRenderModelResponse = { "kind": "model", requestSeq: number, resultRevision: Id, observedAt: Timestamp | null, completeness: ResultCompleteness, model: VizModel, };
export type VizRenderResponse = VizRenderModelResponse | VizRenderNonModelResponse;
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
export type VizGroupRowsRequest = { modelRef: string, groupRef: string, start: number, count: number, };
export type VizSizeRange = { "kind": "known", min: SizeValue, max: SizeValue, } | { "kind": "unknown" };
export type VizRows = { rows: Array<VizRow>, totalCount: number, resultRevision: Id, observedAt: Timestamp | null, completeness: ResultCompleteness, };
export type VizGroupRows = { rows: Array<VizRow>, totalCount: number, groupTotal: SizeValue, sizeRange: VizSizeRange, resultRevision: Id, observedAt: Timestamp | null, completeness: ResultCompleteness, };
export type VizLocateRequest = {
/**
 * Opaque reference returned by rendering the model.
 */
modelRef: string,
/**
 * Item to locate, bound to its original result revision.
 */
itemRef: ItemRef, };
export type VizLocate = { "location": "shape", shapeIndex: number, } | { "location": "group", groupRef: string, } | { "location": "outside_view", } | { "location": "outside_scope", };
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
export type DataSource = "real" | "sample";
export type ResultVersionChanged = { lineageId: Id, sourceRef: Id, origin: string,
/**
 * Data source of the lineage's versions; absent means a sample (F32 FR2.HD1).
 *
 * Sample results never publish this notice, so producers send `real` today.
 * Consumers must not use this value to decide which lineages they track.
 */
dataSource?: DataSource, sequence: number, change: ResultVersionChange, previousResultVersion?: Id, completeness?: PublishedCompleteness, observedAt?: Timestamp, };
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
export type RecentScopeListRequest = Record<string, never>;
export type RecentScopeListRow = { recentScopeRef: Id, includeCount: number, lastUsedAt: Timestamp, };
export type RecentScopeList = { rows: Array<RecentScopeListRow>, };
export type ScopeCorrespondence = { path: OsPath, access: TargetAccess, reasonKey: string | null, };
export type ScopeReview = { scope: ScopeDraft, notices: Array<string>, targets: Array<ScopeCorrespondence>, reviewFingerprint: string, observedAt: Timestamp, requestSeq: number, };
export type RecentScopeOpenRequest = { recentScopeRef: Id, requestSeq: number, };
export type RecentScopeTargetState = "matched" | "moved" | "disconnected" | "unverifiable";
export type RecentScopeOpenTarget = { path: OsPath, state: RecentScopeTargetState, proposedEntry: OsPath | null, };
export type RecentScopeOpen = { draft: ScopeDraft, targets: Array<RecentScopeOpenTarget>, review: ScopeReview | null, };
export type ScanStartRequest = { draft: ScopeDraft, reviewFingerprint: string, };
export type ScanStart = { jobRef: Id, existing: boolean, scopeVersion: Id, scopeFingerprint: string, lineageId?: Id, };
export type ScanFailureKind = "collection_failed" | "unread_range" | "uncertain" | "untraversed_link";
export type ScanFailureStage = "metadata" | "open_directory" | "enumerate" | "traverse";
export type ScanFailurePathRef = { rootIndex: number, relative: Array<PathBytes>, };
export type ScanFailureRow = { kind: ScanFailureKind, stage: ScanFailureStage, pathRef: ScanFailurePathRef, reasonKey: string, observedAt?: Timestamp, displayPath?: string, };
export type ScanFailuresRequest = { resultVersion: Id, kind?: ScanFailureKind, cursor?: string, count?: number, };
export type ScanFailures = { resultVersion: Id, rows: Array<ScanFailureRow>, totalCount: number, nextCursor?: string, };
export type ScanPriorityRequest = {
/**
 * Job reference returned when the scan was admitted.
 */
jobRef: Id,
/**
 * Existing item reference selected from the scan's current partial result.
 */
priorityArea: ItemRef, };
export type ScanPriorityResponse = {
/**
 * The existing job-control result vocabulary.
 */
controlResult: JobControlOutcome,
/**
 * Present when the request remains the applied priority area.
 */
appliedPriorityArea?: ItemRef, };
export type JobState = "queued" | "running" | "pause_requested" | "paused" | "cancel_requested" | "done" | "partial" | "failed" | "cancelled";
export type JobControl = "pause" | "resume" | "cancel" | "set_priority";
export type AnalysisJobRequest = { jobRef: Id, };
export type AnalysisJobListRequest = {
/**
 * Omission selects all states; an empty list selects none.
 */
states?: Array<JobState>, };
export type AnalysisJobPriority = "high" | "normal" | "low";
export type AnalysisJobWaitReason = "none" | "paused" | "queue_full" | "io_wait" | "device_cap" | "backpressure" | "unknown";
export type AnalysisJobListRow = { jobRef: Id, kind: string, scopeSummary: string, state: JobState, waitReason: AnalysisJobWaitReason, priority: AnalysisJobPriority,
/**
 * Supported controls for the job kind, independent of state eligibility.
 */
supportedControls: Array<JobControl>, sequence: number,
/**
 * Present only after the terminal record's history outcome is confirmed.
 */
terminal?: AnalysisJobTerminal, };
export type JobResyncReason = "subscriber_queue_limit" | "subscriber_delivery_failed";
export type JobResyncRequired = { reason: JobResyncReason, discardedEvents: number, observedAt: Timestamp, };
export type ProgressUnit = "items" | "bytes";
export type AnalysisJobProgress = {
/**
 * Per-job sequence shared with state changes and the terminal event.
 */
sequence: number,
/**
 * The progress unit declared by the job kind.
 */
unit: ProgressUnit,
/**
 * Units processed so far.
 */
completed: number,
/**
 * The confirmed total, omitted while the total is not established.
 */
total?: number,
/**
 * Whether `total` is confirmed.
 */
totalKnown: boolean,
/**
 * The wait reason observed with this value.
 */
waitReason: AnalysisJobWaitReason,
/**
 * UTC time at which the runtime observed this value.
 */
observedAt: Timestamp, };
export type ScanResult = { lineageId: Id, sequence: number, resultVersion?: Id, };
export type AnalysisJobTerminal = { state: JobState,
/**
 * Per-job sequence of the terminal event.
 */
sequence: number, completed: number, overwritten: number, failed: number, skipped: number, unprocessed: number,
/**
 * Whether the terminal summary is present in the history store.
 */
historyRecorded: boolean, reasonKey?: string, unprocessedSummary?: string,
/**
 * Immutable analysis version, if the job produced one.
 */
resultVersion?: Id, queryRef?: Id,
/**
 * History storage warning, absent when recording succeeded or was skipped.
 */
warning?: string, };
export type AnalysisJobStatus = { jobRef: Id, kind: string, state: JobState, sequence: number, resultVersion: Id | null,
/**
 * Published query result for a successfully completed query evaluation.
 */
queryRef?: Id, reasonKey: string | null, scanResult?: ScanResult,
/**
 * Confirmed terminal outcome, omitted while history handling is pending.
 */
terminal?: AnalysisJobTerminal, };
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
export type BrowseContext = { origin: ResultOrigin,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, location: ItemRef | null, breadcrumbs: Array<BrowseBreadcrumb>, rootKind: BrowseRootKind, currentCorrespondence: boolean, sortStrategies: Array<QueryStrategy>, predicateStrategies: Array<QueryStrategy>, };
export type BrowseOpenResponse = { "kind": "opened", requestSeq: number, origin: ResultOrigin,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, location: ItemRef | null, breadcrumbs: Array<BrowseBreadcrumb>, rootKind: BrowseRootKind, currentCorrespondence: boolean, page: FolderQueryPage, sortStrategies: Array<QueryStrategy>, predicateStrategies: Array<QueryStrategy>, } | { "kind": "job", requestSeq: number, jobRef: Id,
/**
 * Source result version pinned when the request was admitted.
 */
resultVersion: Id, };
export type BrowseDetailRequest = { resultVersion: Id, item: ItemRef, };
export type BrowseDetail = { item: ItemRef, displayPath: BrowseValue<string>, displayName: BrowseValue<string>, kind: BrowseValue<ResultItemKind>, logicalSize: BrowseValue<SizeValue>, allocatedSize: BrowseValue<SizeValue>, modifiedAt: BrowseValue<Timestamp>, createdAt: BrowseValue<Timestamp>, observedAt: BrowseValue<Timestamp>, origin: BrowseOrigin,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, completeness: ResultCompleteness, currentCorrespondence: boolean, observationOnly: boolean, };
export type BrowseCopyPathRequest = { resultVersion: Id, items: Array<ItemRef>, };
export type BrowseCopyPathRefusal = "path.unrepresentable" | "path.multiline" | "clipboard.unsupported" | "clipboard.failed";
export type BrowseCopyPathOutcome = { "outcome": "copied", count: number, } | { "outcome": "refused", reasonKey: BrowseCopyPathRefusal, };
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
export type SampleSessionState = "preparing" | "active" | "resetting" | "unavailable" | "closed";
export type SampleEventState = "inactive" | "loading" | "active" | "failed";
export type SampleOpenRequest = Record<symbol, never>;
export type SampleSessionRequest = { sessionRef: Id, };
export type SampleObservationRef = {
/**
 * Zero-based observation order in the bundle.
 */
index: number, resultVersion: Id, observedAt?: Timestamp, };
export type SampleSession = { sessionRef: Id, state: SampleSessionState,
/**
 * Monotonic per session; equals the sequence of the latest state event.
 */
sequence: number, bundleVersion?: string, jobRef?: Id,
/**
 * Sample result versions in observation order; empty until loading completes.
 */
observations: Array<SampleObservationRef>,
/**
 * Sample plan versions kept in this session.
 */
plans: Array<Id>, unavailableReason?: string, realAnalysisAction?: string, };
export type SampleClosed = { sessionRef: Id, state: SampleSessionState, releasedVersions: number, };
export type SampleStateChanged = { sessionRef: Id, state: SampleEventState, previousState: SampleEventState, reasonKey?: string, jobRef?: Id, sequence: number, observedAt: Timestamp, };
export type GroupRuleReview = "ok" | "converted" | "predicate_unregistered";
export type GroupRule = { groupId: Id, displayName: string, displayOrder: number, condition: QueryCondition, conditionModelVersion: number, predicates: Array<QueryStrategy>, review: GroupRuleReview, };
export type GroupRuleDraft = { groupId?: Id, displayName: string, condition: QueryCondition, };
export type GroupRuleset = { rulesetVersion: number, revision: number,
/**
 * Lower-case hexadecimal fingerprint of the canonical rule content.
 */
fingerprint: string, updatedAt?: Timestamp, rules: Array<GroupRule>, };
export type GroupRulesGetRequest = Record<symbol, never>;
export type GroupRulesSaveRequest = { expectedRevision: number, rules: Array<GroupRuleDraft>, };
export type GroupRuleIssue = { index: number, reasonKey: string, };
export type GroupRulesSaveResponse = { "outcome": "saved", ruleset: GroupRuleset, } | { "outcome": "rejected", reasonKey: string, issues: Array<GroupRuleIssue>, };
export type GroupRuleSource = { "kind": "saved", rulesetVersion: number, } | { "kind": "draft", rules: Array<GroupRuleDraft>, };
export type GroupClassifyRequest = {
/**
 * GUI view channel; a larger sequence on the same channel cancels earlier work.
 */
viewRef: Id, requestSeq: number, resultVersion?: Id, scope?: ItemRef, rules: GroupRuleSource, };
export type GroupSummaryRow = { groupId: Id, displayName: string, displayOrder: number, memberCount: number,
/**
 * Members that also matched another group.
 */
overlapMemberCount: number,
/**
 * Files this rule could not decide.
 */
undecidableCount: number,
/**
 * The rule uses a predicate that is not registered.
 */
ruleUnevaluable: boolean, };
export type GroupReasonCount = { reasonKey: string, count: number, };
export type GroupLimitation = { key: string, count?: number, };
export type GroupSummary = { classificationRef: Id, resultVersion: Id,
/**
 * Result origin of the pinned version, as published by browse and search.
 */
origin: ResultOrigin,
/**
 * Data source of the pinned version, always set by the producer; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, observedAt?: Timestamp, completeness: ResultCompleteness,
/**
 * Classified folder; absent when the whole result (every scope root) was classified.
 */
scope?: ItemRef,
/**
 * Zero when the rules came from an unsaved draft.
 */
rulesetVersion: number, rulesetFingerprint: string, referenceTime?: Timestamp, groups: Array<GroupSummaryRow>, scopeFileCount: number, matchedCount: number, overlapCount: number, unclassifiedCount: number, undecidableCount: number,
/**
 * Whether scope = matched + unclassified + undecidable held at registration.
 */
reconciled: boolean, undecidableReasons: Array<GroupReasonCount>, limitations: Array<GroupLimitation>, predicates: Array<QueryStrategy>, };
export type GroupClassifyResponse = { "kind": "summary", requestSeq: number, summary: GroupSummary, } | { "kind": "job", requestSeq: number, jobRef: Id, classificationRef: Id, resultVersion: Id,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, } | { "kind": "not_applicable", requestSeq: number, reasonKey: string, };
export type GroupSummaryRequest = { classificationRef: Id, };
export type GroupCancelRequest = { "kind": "view", viewRef: Id, requestSeq: number, } | { "kind": "job", jobRef: Id, };
export type GroupCancelOutcome = "accepted" | "no_effect" | "already_terminal";
export type GroupCancelResponse = { outcome: GroupCancelOutcome, };
export type GroupCategory = { "kind": "group", groupId: Id, } | { "kind": "overlap", } | { "kind": "unclassified", } | { "kind": "undecidable", };
export type GroupMembersRequest = { classificationRef: Id, resultVersion: Id, category: GroupCategory,
/**
 * Defaults to descending logical size; the path order always breaks ties.
 */
sort?: BrowseSortKey, start: number, count: number, };
export type GroupMemberRow = { item: ItemRef, displayName: BrowseValue<string>,
/**
 * Original observed path; a group name never appears here.
 */
displayPath: BrowseValue<string>, logicalSize: BrowseValue<SizeValue>, allocatedSize: BrowseValue<SizeValue>,
/**
 * Matched groups in rule display order.
 */
groups: Array<Id>, undecidableReasonKeys: Array<string>, };
export type GroupMembers = { classificationRef: Id, resultVersion: Id,
/**
 * Data source of the pinned version, always set by the producer; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, start: number, total: number, rows: Array<GroupMemberRow>, sort: QueryStrategy, };
export type GroupExplainRequest = { classificationRef: Id, item: ItemRef, };
export type GroupDecision = "match" | "no_match" | "undecidable";
export type GroupConditionNode = { path: Array<number>, predicate?: QueryStrategy, decision: GroupDecision, };
export type GroupRuleDecision = { groupId: Id, decision: GroupDecision, reasonKey?: string, nodes: Array<GroupConditionNode>, };
export type GroupExplanation = { classificationRef: Id, item: ItemRef,
/**
 * Data source of the pinned version, always set by the producer; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, displayPath: BrowseValue<string>, observedAt?: Timestamp, rules: Array<GroupRuleDecision>, };
export type GroupTotalsRequest = { classificationRef: Id, groupIds: Array<Id>, };
export type GroupTotal = { sizes: Array<SpaceTotalSize>, completeness: ResultCompleteness, };
export type GroupTotalRow = { groupId: Id, total: GroupTotal, };
export type GroupRatio = { "state": "value", basis: SizeBasis, numerator: ByteSize, denominator: ByteSize, } | { "state": "undefined", basis: SizeBasis, reasonKey: string, };
export type GroupTotals = { classificationRef: Id, resultVersion: Id,
/**
 * Data source of the pinned version, always set by the producer; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, groups: Array<GroupTotalRow>, union: GroupTotal, overlap: GroupTotal, overlapCount: number, unclassified: GroupTotal, undecidable: GroupTotal, scope: GroupTotal, ratios: Array<GroupRatio>, sizeRule: QueryStrategy, };
export const SIZE_UNIT_RULES = {"decimalPlaces":1,"iec":{"base":1024,"symbols":["B","KiB","MiB","GiB","TiB","PiB"]},"revision":1,"si":{"base":1000,"symbols":["B","kB","MB","GB","TB","PB"]}} as const;
