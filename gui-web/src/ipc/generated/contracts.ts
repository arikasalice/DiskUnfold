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
appLocation?: StartupAppLocation,
/**
 * The startup notice state key the user dismissed, when one is remembered.
 *
 * Omitted when nothing is remembered. Read when the status is answered, so it
 * follows the latest `app.startup_memory_set`, also while the app is closing.
 */
noticeDismissed?: string,
/**
 * The state key of the Support information the user has seen, when remembered.
 *
 * Omitted when nothing is remembered. Read when the status is answered, so it
 * follows the latest `app.startup_memory_set`, also while the app is closing.
 */
reviewSeen?: string, };
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
export type StartupMemoryKind = "notice_dismissed" | "review_seen";
export type StartupMemorySetRequest = {
/**
 * The memory to update.
 */
kind: StartupMemoryKind,
/**
 * The state key to remember.
 */
stateKey: string, };
export type StartupMemorySetResponse = {
/**
 * The memory that was updated.
 */
kind: StartupMemoryKind,
/**
 * The stored state key; empty when nothing is remembered.
 */
stateKey: string, };
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
 * Operating-system family of the host returning this profile; older hosts omit it.
 */
osFamily?: SupportOsFamily,
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
export type HistoryListCounts = {
/**
 * "0" when the job ended without a result: its counted units are then
 * in `unprocessed`, so `completed + failed` can be lower than the last
 * progress value the job reported.
 */
completed: string, overwritten: string, failed: string, skipped: string, unprocessed: string, };
export type HistoryListSummary = { kindId: string, kindVersion: number, scopeSummary: string | null,
/**
 * Omitted when roots were not recorded (an older record or a kind without scope roots).
 */
scopeFirstRoot?: string,
/**
 * Omitted when roots were not recorded (an older record or a kind without scope roots).
 */
scopeRootCount?: number, state: HistoryEndState, counts: HistoryListCounts | null, startedAtMs: number | null, finishedAtMs: number | null, predecessorJobRef: Id | null, relation: HistoryRelation | null, sessionRestoreAvailable: boolean | null, };
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
export type HistoryDetail = { summary: HistoryListSummary | null,
/**
 * Ordered root display strings. Omitted when roots were not recorded (an older record or a kind without scope roots).
 */
scopeRoots?: Array<string>, workflow: HistoryWorkflow, followUps: Array<HistoryFollowUp> | null, effects: HistoryEffects, completeness: HistoryDetailCompleteness, missingReasons: Array<string>, };
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
export type HistoryItemRecording = "recorded" | "not_recorded";
export type HistoryItemsResponse = { revision: HistoryListRevision, rows: Array<HistoryItemRow>, counts: HistoryItemCounts,
/**
 * Separates an operation that never records items from one that recorded none.
 *
 * The engine always sets it. Consumers read an omitted or unknown value as
 * unknown and fall back to `counts.summary`: null means not recorded.
 */
itemRecording?: HistoryItemRecording, completeness: HistoryDetailCompleteness, missingReasons: Array<string>, nextCursor: string | null, };
export type HistoryRetentionGetRequest = Record<symbol, never>;
export type HistoryRetentionPolicy = {
/**
 * Whether automatic retention is enabled.
 */
enabled: boolean,
/**
 * The user-specified age limit, omitted when no override is stored.
 */
specifiedDays?: number,
/**
 * Built-in suggestion, which is never stored as a setting.
 */
suggestedDays: number,
/**
 * Lowercase hexadecimal digest that identifies this policy and its read state.
 */
policyFingerprint: string,
/**
 * Stable reason key when the stored policy could not be trusted.
 */
unavailableReason?: string, };
export type HistoryRetentionChangeKind = "none" | "enable" | "shorten" | "extend" | "disable" | "set_period";
export type HistoryRetentionReviewRequest = { enabled: boolean, days?: number, };
export type HistoryRetentionApplyRequest = { reviewRef: Id, fingerprint: string, };
export type HistoryRetentionCancelRequest = { reviewRef: Id, };
export type HistoryRetentionReview = { reviewRef: Id, changeKind: HistoryRetentionChangeKind, confirmationRequired: boolean, cleanupAfterApply: boolean, targetCount: string, targetEarliestAtMs?: number | null, targetLatestAtMs?: number | null, heldCount: string, undoInfoRemovalCount: string, fingerprint: string, };
export type HistoryRetentionApplyOutcome = "applied" | "no_effect";
export type HistoryRetentionApplyResponse = { outcome: HistoryRetentionApplyOutcome, policy: HistoryRetentionPolicy, cleanupJobRef?: Id, };
export type HistoryRetentionCancelOutcome = "cancelled" | "no_effect";
export type HistoryRetentionCancelResponse = { outcome: HistoryRetentionCancelOutcome, };
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
export type VizAvailabilityRequest = { sourceRef?: VizSourceRef, resultRevision: Id, aggregateScopeRef: ItemRef, };
export type VizFormatAvailability = { format: string, available: boolean,
/**
 * Null for an available format; required and non-null for an unavailable one.
 */
reasonKey: string | null, };
export type VizAvailabilityResponse = { formats: Array<VizFormatAvailability>, requiredDataTableRevision: number, };
export type VizCatalogRequest = Record<string, never>;
export type VizCatalog = { formats: Array<string>, strategies: Array<QueryStrategy>, measures: Array<string>, colorCriteria: Array<string>, denominatorCriteria: Array<string>, interpretationTableRevision: number, requiredDataTableRevision: number, legendRuleRevision: number,
/**
 * Revision of the type classification table behind the "type" color
 * criterion; absent when the catalog offers no type criterion.
 */
fileTypeTableRevision?: number, };
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
export type VizTotals = { scope: VizTotal, drawn: VizTotal, grouped: VizTotal, filteredOut: VizTotal,
/**
 * What the scope holds beyond `drawn`, `grouped` and `filteredOut`.
 *
 * Siblings below the minimum area or coordinate precision, and space no single item
 * can carry, such as a file shared between siblings that the scope counts once. The
 * engine computes it; a consumer only displays it. Exact only when the scope and the
 * other three totals are exact, and then never negative; otherwise unknown with a
 * reason. Omitted rather than null; a model without it says nothing about it.
 */
remainder?: VizTotal,
/**
 * What the remainder holds: `viz.remainder.too_small` or `viz.remainder.unattributed`.
 *
 * Omitted when the remainder is absent or exactly zero, at least one key when it is
 * exact and not zero, and the known causes or nothing when it is unknown (its reason
 * says why). Valid only with `remainder`; keys are not empty, sorted and without
 * duplicates, and an unregistered key is preserved. Omitted rather than null.
 */
remainderReasonKeys?: Array<string>,
/**
 * The F03 measurement limits of `scope` (#351): the `measurementLimits` that
 * `space.total` gives the same item in the model's size basis, from the same
 * function, so the two always agree. A model that does not measure size has none.
 *
 * Only the scope total has them: the other totals are sums F06 makes, which do
 * not inherit F03's limits. Omitted when there is none, never empty or null; keys
 * are not empty or repeated, keep F03's order, and an unknown key is preserved.
 */
scopeMeasurementLimits?: Array<string>, };
export type VizTableRevisions = { interpretation: number, requiredData: number, legend: number, viewPolicy: number, sizeRule: QueryStrategy,
/**
 * Revision of the type classification table, present when the model's
 * legend classes come from it (color criterion "type").
 */
fileType?: number, };
export type VizMatchKey = { "state": "present", value: CorrespondenceKey, } | { "state": "unknown", reasonKey: string, } | { "state": "not_applicable", };
export type VizItemKind = "file" | "folder" | "other";
export type VizShape = { itemRef: ItemRef | null, groupRef: string | null, matchKey: VizMatchKey, geometry: VizGeometry, size: SizeValue, quantity?: VizQuantity, legendClassKey: string, displayFlags: Array<string>, shapeRef?: string, parentShapeRef?: string | null, visibleDepth?: number, rootBranchRef?: ItemRef | null,
/**
 * Number of members of a group shape, in items.
 *
 * Present only on a group shape (a group reference and no item reference), with the
 * unit items; the engine puts it on every group shape it makes. The same count is on
 * the group's row and, when exact, equals the total count of the group's
 * `viz.group_rows`. A lower bound or an unknown count is never replaced by zero. The
 * field is omitted rather than null, and a model without it is valid.
 */
memberCount?: VizQuantity,
/**
 * Kind of the referenced item, on item shapes only; omitted when unknown. See
 * [`VizItemKind`].
 */
itemKind?: VizItemKind,
/**
 * The files by legend class of a folder whose children are not drawn, or of the
 * Other group by its members; the same list as its row's. See [`VizTypeSlice`].
 */
typeSlices?: Array<VizTypeSlice>, };
export type VizLegendEntry = { key: string, labelKey: string, itemCount: VizQuantity, total: VizTotal, condition: QueryCondition | null, };
export type VizSummary = { groupedItems: VizQuantity, omittedItems: VizQuantity, unknownItems: VizQuantity, reasonKeys: Array<string>,
/**
 * Bytes of the scope measured under navigation-root items without a provable area
 * and outside every drawn item (F06 README 51.3). Treemap only; omitted when no
 * such item holds a measured byte, and never an exact zero.
 */
unknownSize?: VizQuantity,
/**
 * Position in the navigation root's table (viz.rows without parentShapeRef) of the
 * first row without a provable area, or else of the first expanded folder whose
 * level has one. Treemap only; omitted when no table row is such a row.
 */
firstUnknownRow?: number,
/**
 * Folders below the model's scope whose listing failed (F06 README 48, #346): a
 * recorded scan gap, or a folder a cancellation or disconnection left unread. Exact
 * from the recorded gaps; a lower bound while the scan runs or when such marks make
 * the count incomplete. Omitted when there are none, never an exact zero. These
 * folders are not in `unknownItems`.
 */
unreadFolders?: VizQuantity,
/**
 * Position in the navigation root's table of the first row that is such a folder
 * or holds one. Omitted with `unreadFolders`, or when no table row holds one.
 */
firstUnreadRow?: number, };
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
export type VizRow = { itemRef: ItemRef | null, groupRef: string | null,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayName: string, size: SizeValue, quantity?: VizQuantity, ratio: VizRatio | null, denominator: VizDenominator, legendClassKey: string, displayFlags: Array<string>,
/**
 * Message-bundle key for a group row the engine made, such as the merged small
 * siblings of a treemap.
 *
 * Present only on such rows; the value is a key registered in the message
 * bundle, never a path or file name. A consumer that understands it shows that
 * message and does not use the display name as the row's name. The display name
 * is required and carries the same key string, as a fallback for a consumer that
 * does not know this field. Item rows, and group rows whose name the supplier
 * gave (for example a histogram bin), omit it. An empty string is invalid; the
 * field is omitted rather than null. A row that has a label key must be a group
 * row, with a group reference and no item reference; an item row that carries
 * one is invalid.
 */
labelKey?: string,
/**
 * Number of members of a group row, in items.
 *
 * Present only on a group row, never on an item row; a row that has one must have
 * a group reference and no item reference, and its unit must be items. The engine
 * puts it on every group row it makes. An exact count equals the total count of
 * that group's `viz.group_rows`. A lower bound or an unknown count (with its reason)
 * is reported as such and is never replaced by zero. The field is omitted rather
 * than null, and a response without it is valid.
 */
memberCount?: VizQuantity,
/**
 * Kind of the referenced item, on item rows only; omitted when unknown. See
 * [`VizItemKind`].
 */
itemKind?: VizItemKind,
/**
 * The list condition of a histogram interval row (F06 README 12, #297): its
 * files in the half-open size interval [lower, upper) on the model's size basis,
 * as All[Type file, Size at least lower, Size less than upper]; the last interval
 * has no upper bound. Only on group rows; omitted on every other row, and where
 * the size basis has no size predicate (unique physical size). Omitted rather
 * than null.
 */
condition?: QueryCondition,
/**
 * Whether this folder could not be read (F06 README 53): item rows of folders
 * only. See [`VizUnread`].
 */
unread?: VizUnread,
/**
 * The files by legend class (F06 README 54) of a folder row, or of the Other
 * group row by its members together, under the type color criterion, unfiltered,
 * in a real scan. Omitted where it does not apply, including a folder whose own
 * listing failed; empty when every file was counted and there is none. See
 * [`VizTypeSlice`].
 */
typeSlices?: Array<VizTypeSlice>, };
export type VizUnread = { "kind": "itself" } | { "kind": "inside",
/**
 * Unread ranges recorded below the folder, a positive u64 in decimal; omitted
 * when a cancellation or disconnection left folders below it uncounted.
 */
count?: string, } | { "kind": "unknown" };
export type VizTypeSlice = { classKey: string, itemCount: VizQuantity, total: VizQuantity, };
export type VizRowsRequest = { modelRef: string, resultRevision: Id, start: number, count: number,
/**
 * The shape of an expanded folder in this model, to read that folder's rows.
 *
 * Omitted, the page is the model's root table, as before. Present, it names a
 * shape of the same model, which must be a folder that was expanded: the page then
 * holds the drawn children of that folder and the row of its group, if it has
 * one, and `totalCount` counts that table. A reference the model does not have
 * is refused as not found, and a shape that is no expanded folder (a file, a
 * group, a folder that was not expanded) as an invalid argument. Rows and shapes
 * correspond by item reference or group reference, never by position. An empty
 * string is invalid and the field is omitted rather than null.
 */
parentShapeRef?: string, };
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
export type OsPath = { encoding: PathEncoding, raw: PathBytes,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
display: string, lossy: boolean, };
export type ItemRef = { resultVersion: Id, itemNo: number, };
export type ErrorCode = "AUTH_REQUIRED" | "ORIGIN_DENIED" | "SCOPE_DENIED" | "INVALID_ARGUMENT" | "NOT_FOUND" | "STALE_VERSION" | "BUSY" | "INTERNAL";
export type ErrorEnvelope = { code: ErrorCode, reasonKey: string, messageKey: string, correlationId: Id, itemRefs?: Array<ItemRef>, retryable: boolean, };
export type CallEnvelope = { name: string, contractVersion: number, correlationId: Id, body: unknown, };
export type ResultEnvelope = { name: string, contractVersion: number, correlationId: Id, body: unknown, error?: ErrorEnvelope, };
export type EventEnvelope = { topic: string, contractVersion: number, seq: number, correlationId?: Id, jobRef?: Id, resultVersion?: Id, occurredAt: Timestamp, body: unknown, };
export type ResultVersionChange = "published" | "withdrawn";
export type PublishedCompleteness = "complete" | "partial" | "unknown";
export type DataSource = "real" | "snapshot" | "sample";
export type ResultVersionChanged = { lineageId: Id, sourceRef: Id, origin: string,
/**
 * Data source of the lineage's versions; absent means a sample (F32 FR2.HD1).
 *
 * Sample results never publish this notice, so producers send `real` today.
 * Consumers must not use this value to decide which lineages they track.
 */
dataSource?: DataSource, sequence: number, change: ResultVersionChange, previousResultVersion?: Id, completeness?: PublishedCompleteness, observedAt?: Timestamp, };
export type FreshnessState = string;
export type FreshnessCounts = { observed: number, unwatched: number, change_notified: number, gap_unknown: number, refreshing: number, unverifiable: number, };
export type FreshnessSummary = {
/**
 * The worst state of all rows.
 *
 * A summary without rows has no worst state, and its representative is
 * `unverifiable`; a receiver never assumes `observed` there. A receiver first
 * reads an event without a current version and with every count zero as the removal
 * of that scope version's ledger.
 */
representative: FreshnessState, counts: FreshnessCounts, };
export type FreshnessRange = {
/**
 * Position in the root list of the event's scope version.
 */
rootIndex: number, state: FreshnessState,
/**
 * An existing reason key explaining the state, such as a scope gap's reason.
 */
reasonKey?: string, };
export type FreshnessChanged = { "kind": "ranges", ranges: Array<FreshnessRange>, } | { "kind": "requery" } | { "kind": "unknown" };
export type ResultFreshnessChanged = {
/**
 * The lineage whose `results.version_changed` events the receiver tracks.
 */
lineageId: Id,
/**
 * The scope version whose ledger changed; `rootIndex` refers to its root list.
 */
scopeVersionId: Id,
/**
 * Rises by one for each change of this scope version's ledger, from 1, below 2^53.
 * It is not comparable with the lineage sequence of `results.version_changed`.
 */
sequence: number, summary: FreshnessSummary, changed: FreshnessChanged,
/**
 * The observation time of the result version that caused this change. Omitted when
 * the change does not come from an observation (a refresh start or a withdrawal) or
 * that version has no observation time.
 */
observedAt?: Timestamp, };
export type TargetKind = "internal" | "external" | "network_mount" | "folder_mount";
export type IdentitySource = "confirmed" | "estimated" | "unavailable";
export type TargetAccess = "checking" | "accessible" | "denied" | "disconnected" | "missing" | "unresponsive" | "unknown";
export type TargetListRequest = { refresh: boolean, };
export type AnalysisTarget = { kind: TargetKind | null, path: OsPath, connected: boolean, volumeId: string | null, identitySource: IdentitySource, access: TargetAccess, reasonKey: string | null,
/**
 * Whether the target belongs in the default list (F01 #263). The engine always
 * sets it; absent means listed. It only arranges the list and never limits a scope.
 */
defaultVisible?: boolean,
/**
 * The OS-confirmed system volume of a confirmed root volume group.
 * Omitted when the group or system role is not confirmed. GUI channel only.
 */
isStartupVolume?: boolean,
/**
 * Volume name reported by the operating system, when available. GUI channel only.
 */
volumeName?: string, };
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
export type ScopeVolumeGroup = {
/**
 * Mount path of the data volume, counted together with `/`.
 */
dataMount: OsPath, };
export type ScopeReview = { scope: ScopeDraft, notices: Array<string>, targets: Array<ScopeCorrespondence>, reviewFingerprint: string, observedAt: Timestamp, requestSeq: number,
/**
 * The macOS root volume group the reviewed scope analyses as one volume (#263).
 * Omitted when the scope is not a volume group. GUI channel only.
 */
volumeGroup?: ScopeVolumeGroup,
/**
 * The user's home folder, offered only with
 * [`ScopeVolumeGroup::NOTICE_UNVERIFIED`] so the user can pick their own files
 * in one step (#263). A suggestion, not a permission: choosing it replaces the
 * draft and the new draft is reviewed as usual. It never changes a fingerprint.
 * Omitted otherwise; `null` is rejected. GUI channel only.
 */
suggestedInclude?: OsPath, };
export type RecentScopeOpenRequest = { recentScopeRef: Id, requestSeq: number, };
export type RecentScopeTargetState = "matched" | "moved" | "disconnected" | "unverifiable";
export type RecentScopeOpenTarget = { path: OsPath, state: RecentScopeTargetState, proposedEntry: OsPath | null, };
export type RecentScopeOpen = { draft: ScopeDraft, targets: Array<RecentScopeOpenTarget>, review: ScopeReview | null, };
export type ScanStartRequest = { draft: ScopeDraft, reviewFingerprint: string,
/**
 * Resource controls are not part of the reviewed scope fingerprint.
 */
collection?: ScanCollectionCondition, };
export type ScanStart = { jobRef: Id, existing: boolean, scopeVersion: Id, scopeFingerprint: string,
/**
 * The current resource condition of this job, including on duplicate starts.
 */
collection?: ScanCollection, lineageId?: Id, };
export type ScanResourceMode = "auto" | "quiet" | "fast";
export type DeviceKind = "ssd" | "hdd" | "unknown";
export type DeviceKindSource = "detected" | "user" | "unknown";
export type ScanCollectionCondition = {
/**
 * Omission selects `auto`.
 */
resourceMode?: ScanResourceMode,
/**
 * Omission leaves the strategy's worker limit unrestricted by the user.
 */
userLimit?: number, };
export type ScanCollection = { resourceMode: ScanResourceMode, userLimit?: number, };
export type ScanLaneStrategy = {
/**
 * Receivers treat an unknown ID as an unknown strategy.
 */
id: string, version: number, };
export type ScanSetResourceRequest = { jobRef: Id, resourceMode: ScanResourceMode,
/**
 * Omission clears the prior user limit.
 */
userLimit?: number, };
export type ScanSetResourceLane = { laneIndex: number, effectiveLimit: number,
/**
 * Device permit by kind; it keeps the full per-device permit when lanes share a disk.
 */
permit?: number,
/**
 * This lane's share of a permit split with other lanes on the same disk.
 */
share?: number, };
export type ScanSetResourceOutcome = "accepted" | "no_effect" | "already_terminal";
export type ScanSetResourceResponse = { outcome: ScanSetResourceOutcome, lanes: Array<ScanSetResourceLane>,
/**
 * An unknown value is interpreted as `next_assignment` by receivers.
 */
appliesFrom: string, };
export type ScanFailureKind = "collection_failed" | "unread_range" | "uncertain" | "untraversed_link";
export type ScanFailureStage = "metadata" | "open_directory" | "enumerate" | "traverse";
export type ScanFailurePathRef = { rootIndex: number, relative: Array<PathBytes>, };
export type ScanFailureRow = { kind: ScanFailureKind, stage: ScanFailureStage, pathRef: ScanFailurePathRef, reasonKey: string, observedAt?: Timestamp,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayPath?: string, };
export type ScanFailuresRequest = { resultVersion: Id, kind?: ScanFailureKind,
/**
 * Restricts rows to these zero-based root indexes. Omission selects all roots.
 * The list must contain 1 to 64 strictly ascending indexes, all below the
 * pinned result's root count; `null`, empty, duplicate, descending, and
 * oversized lists are invalid.
 */
rootIndexes?: Array<number>,
/**
 * Selects exactly issues counted as lane failures: every `collection_failed` row;
 * `unread_range` rows at `open_directory` or `enumerate`; and traverse-stage unread
 * rows with reason `scan.boundary_unverifiable` or the worker failure reason
 * `scan.unclassified`. Omission is false. Known other-volume and revisit ranges,
 * `uncertain`, and `untraversed_link` rows are excluded. When combined with `kind`,
 * both filters apply.
 */
failuresOnly?: boolean, cursor?: string, count?: number, };
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
export type AnalysisJobListRow = { jobRef: Id, kind: string, scopeSummary: string,
/**
 * Display string of the first scope root captured at submission.
 * Omitted, together with `scope_root_count`, for a kind without scope roots.
 */
scopeFirstRoot?: string,
/**
 * Number of scope roots captured at submission (1 or more).
 * Omitted, together with `scope_first_root`, for a kind without scope roots.
 */
scopeRootCount?: number, state: JobState, waitReason: AnalysisJobWaitReason, priority: AnalysisJobPriority,
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
 * Units processed so far: the completed and the failed count of the job
 * added together (see [`crate::job::JobCounts`]). Each unit is counted
 * once, and the value never decreases within a job. The terminal counts
 * of a job that ends without a result can add up to less than this
 * value: that job reports 0 completed and its counted units as
 * unprocessed.
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
export type ScanLaneWait = "io_wait" | "backpressure" | "device_cap" | "none";
export type ScanLane = {
/**
 * Fixed for the lifetime of the job.
 */
laneIndex: number,
/**
 * The first [`MAX_LANE_ROOTS`] scope root indexes of the lane, ascending.
 */
rootIndexes: Array<number>,
/**
 * All root indexes of the lane as a decimal string; present only when
 * `root_indexes` was cut.
 */
rootCount?: string,
/**
 * OS calls of the lane in flight (u32).
 */
inFlight: number,
/**
 * The lane's effective worker limit (u32, at least 1; 0 is never sent).
 */
limit: number,
/**
 * Device class; omitted or unrecognized values mean `unknown`.
 */
deviceKind?: string,
/**
 * Classification source; omitted or unrecognized values mean `unknown`.
 */
deviceKindSource?: string,
/**
 * Requested resource strategy; omitted when the producer cannot report it.
 */
mode?: ScanResourceMode,
/**
 * Device permit by kind; it keeps the full per-device permit when lanes share a disk.
 */
permit?: number,
/**
 * This lane's share of a permit split with other lanes on the same disk.
 */
share?: number,
/**
 * Strategy identity used to calculate `limit`.
 */
strategy?: ScanLaneStrategy,
/**
 * The same `ItemRef` the GUI sent to `scan.prioritize` and the engine
 * returned as `appliedPriorityArea`; it carries no path or name and is
 * omitted when this lane has no priority area. It remains present while
 * the engine keeps the region, even at zero.
 */
priorityArea?: ItemRef,
/**
 * Known directories in `priority_area` not read to their end, decimal.
 * Zero alone does not release the region.
 */
remainingPrioritizedDirectories?: string,
/**
 * The wait cause; not to be used when `wait_confirmed` is false.
 */
waitReason: ScanLaneWait, waitConfirmed: boolean,
/**
 * Directories known to the lane and not read to their end, decimal.
 */
remainingDirectories: string,
/**
 * Failures recorded for the lane's roots so far, decimal.
 */
failures: string, };
export type ScanLaneStatus = { sequence: number,
/**
 * The first [`MAX_LANES`] lanes by index.
 */
lanes: Array<ScanLane>,
/**
 * All lanes as a decimal string; present only when `lanes` was cut.
 */
laneCount?: string,
/**
 * UTC time at which the runtime observed this value.
 */
observedAt: Timestamp, };
export type ScanResult = { lineageId: Id, sequence: number, resultVersion?: Id, };
export type AnalysisJobTerminal = { state: JobState,
/**
 * Per-job sequence of the terminal event.
 */
sequence: number,
/**
 * 0 when the job ended without a result: its counted units are then in
 * `unprocessed`, so `completed + failed` can be below its last progress.
 */
completed: number, overwritten: number, failed: number, skipped: number, unprocessed: number,
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
export type SpaceObservationRange = { earliest: Timestamp | null, latest: Timestamp | null, complete: boolean, };
export type SpaceTotal = { resultVersion: Id, requestSeq: number, sizes: Array<SpaceTotalSize>, completeness: ResultCompleteness, missingItems: ByteSize | null, inaccessibleRanges: ByteSize | null, untraversedLinks: ByteSize | null,
/**
 * The registered aggregation rule that produced these sizes, read from the pinned
 * result version. Absent when an older producer answered.
 */
sizeRule?: QueryStrategy,
/**
 * The exact number of folders below the target left unread because the scan was
 * cancelled, including the target itself when it is such a folder. Other-volume and
 * already-counted folders, gaps and disconnects are not counted. `0` confirms none only
 * for a sealed real scan with `Complete` or `Missing` status and no cancelled marks.
 *
 * Absent when the source cannot prove the total: a sample, an unfinished publication, a
 * partial or cancelled result (even when it has marks), a complete or missing result
 * containing a cancelled mark, an incremental replacement, or a target that is not a
 * single subtree (a query result).
 * Absence is not zero.
 */
cancelledUnreadFolders?: number,
/**
 * Observation-time bounds of the items this total read in the pinned result version.
 *
 * A new producer sets it for a scope root, a folder and a query result; for a query
 * result it covers the pinned items the set total actually reads. A group total that
 * reuses this body does not carry it. A producer does not write null, and an explicit
 * null decodes as absent. Absent means unknown, never zero or exact.
 */
observationRange?: SpaceObservationRange, };
export type VolumeGapSelection = { volumeId?: PathBytes | null, root?: ItemRef | null, };
export type VolumeGapRequest = { resultVersion: Id, requestSeq: number, target: VolumeGapSelection, };
export type VolumeGapObservation = { "state": "not_attempted" } | { "state": "unavailable", observedAt: Timestamp | null, reasonKey: string, } | { "state": "available", observedAt: Timestamp | null, totalBytes: ByteSize, usedBytes: ByteSize, freeBytes: ByteSize, evidence: SizeEvidence, };
export type VolumeGapSize = { size: SizeValue, completeness: ResultCompleteness, };
export type VolumeGapAmount = { "state": "exact", bytes: ByteSize, evidence: SizeEvidence, } | { "state": "unknown", reasonKey: string, };
export type VolumeGapRow = { key: string, value: VolumeGapAmount, };
export type VolumeGapVolumeRow = {
/**
 * Human-readable volume name as the OS reports it.
 */
volumeName: string,
/**
 * Display role, an open set of keys; see [`VolumeGapVolumeRow::ROLE_VM`]. A
 * receiver shows an unknown or omitted role like `volume_role.other`.
 */
role?: string, amount: VolumeGapAmount, };
export type VolumeGapCategories = { "state": "unavailable", reasonKey: string, } | { "state": "available", rows: Array<VolumeGapRow>, };
export type VolumeGapResponse = { resultVersion: Id, requestSeq: number, volumeId: PathBytes | null, start: VolumeGapObservation, end: VolumeGapObservation, allocated: VolumeGapSize, uniquePhysical: VolumeGapSize, factors: Array<VolumeGapRow>, remainder: VolumeGapAmount, osCategories: VolumeGapCategories,
/**
 * The system volumes sharing the container of a root volume group target,
 * one row each (#263). Their amounts add up to the `gap.out_of_scope` factor.
 * Omitted when the target is not a root volume group.
 */
otherVolumes?: Array<VolumeGapVolumeRow>, };
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
export type FolderBoundary = string;
export type FolderFlags = {
/**
 * Present with `true` when the contents of this folder should have been read but
 * could not be: its open or a read failed, its boundary could not be decided, or
 * the scan was cancelled or its volume was disconnected first. A boundary that
 * could not be decided marks the row of the folder that was not entered, not its
 * parent. A table shows only this flag.
 * A cancelled result alone does not establish why an individual folder was not read.
 *
 * A row never has `unenumerated` present with `true` together with a present
 * `boundary`.
 */
unenumerated: BrowseValue<boolean>,
/**
 * Whether an exclusion rule left this entry out. The collector does not keep
 * excluded entries, so a row of a scan-lineage result is never an excluded entry
 * and a producer may send present with `false`.
 */
excluded: BrowseValue<boolean>,
/**
 * The scope rule that kept this folder from being entered.
 *
 * Present: the folder was not entered because of the scope rules. Absent: the
 * folder is confirmed not to be a boundary. Unknown: it cannot be decided yet.
 * Omitted: the source does not supply this fact, as for a source that is not
 * scan lineage and for file rows.
 */
boundary?: BrowseValue<FolderBoundary>,
/**
 * Where a folder that was `already_counted` through a confirmed firmlink was
 * counted, such as `/Users` for `/System/Volumes/Data/Users` (#263).
 *
 * For navigation only: never add or subtract sizes with it. Present only with a
 * present `already_counted` boundary whose cause is a firmlink of the confirmed
 * group; omitted otherwise. `null` is rejected. GUI channel only.
 */
countedAt?: OsPath,
/**
 * The item that `counted_at` names in the same result version, for moving there
 * (#263). Present only with `counted_at` and only when that folder is in the
 * result; never used for sizes. `null` is rejected. GUI channel only.
 */
countedAtItem?: ItemRef,
/**
 * The folder's place in the result's confirmed root volume group (#263), an
 * open set: [`FolderFlags::GROUP_ROLE_DATA`] marks the data mount itself.
 * Omitted for every other folder and an unknown value means no mark; the
 * screen never compares paths for it. `null` is rejected. GUI channel only.
 */
groupVolumeRole?: string,
/**
 * Unread ranges inside a folder that was itself read (#226).
 *
 * Present: some ranges below the folder were not read; see [`UnreadInside`]. Absent:
 * nothing below was recorded as unread. Unknown: the scan has not ended. Omitted: the
 * fact does not apply, as for a folder that was not read itself (its row shows
 * `unenumerated`), a file row or a source that is not scan lineage.
 */
unreadInside?: BrowseValue<UnreadInside>, };
export type UnreadInside = {
/**
 * Number of unread ranges (gaps, not folders) below the folder, as a decimal string.
 * Omitted when a folder below was left unread by a cancellation or disconnection,
 * which is not counted as a range, so the number is not known.
 */
count?: string, };
export type FolderQueryRow = { item: ItemRef,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayName: BrowseValue<string>, kind: ResultItemKind, logicalSize: BrowseValue<SizeValue>, allocatedSize: BrowseValue<SizeValue>, modifiedAt: BrowseValue<Timestamp>, hasChildFolder: BrowseValue<boolean>, flags: FolderFlags, correspondence: BrowseValue<CorrespondenceKey>, };
export type SearchQueryRow = { item: ItemRef,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayName: BrowseValue<string>,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayPath: BrowseValue<string>, kind: ResultItemKind, logicalSize: BrowseValue<SizeValue>, allocatedSize: BrowseValue<SizeValue>, modifiedAt: BrowseValue<Timestamp>, createdAt: BrowseValue<Timestamp>, missingFields: Array<string>, reasonKeys: Array<string>, };
export type QueryTotal = { "state": "exact", count: number, } | { "state": "lower_bound", count: number, };
export type FolderRowKind = "folder";
export type SearchRowKind = "search";
export type QueryPage<R> = { queryRef: Id, resultVersion: Id, rows: Array<R>, total: QueryTotal, unknownCount: number, observedAt: Timestamp | null, completeness: ResultCompleteness, nextCursor: string | null, };
export type FolderQueryPage = { rowKind: FolderRowKind,
/**
 * Location context for a directly requested first page, when available.
 */
browseContext?: BrowseContext,
/**
 * Why lower bounds on this page have no row reason (#226), as a reason key:
 * `scan.evidence_not_recorded` (analyzing again gives exact values),
 * `scan.evidence_budget_exceeded` or `scan.lower_bound_unexplained`. Present only
 * when a size or the item count on this page is a lower bound that no row mark,
 * unread range, boundary or measurement limit explains, and never while the scan
 * runs. An unknown key reads as `scan.lower_bound_unexplained`.
 */
unexplainedLowerBound?: string, queryRef: Id, resultVersion: Id, rows: Array<FolderQueryRow>, total: QueryTotal, unknownCount: number, observedAt: Timestamp | null, completeness: ResultCompleteness, nextCursor: string | null, };
export type SearchQueryPage = { rowKind: SearchRowKind, queryRef: Id, resultVersion: Id, rows: Array<SearchQueryRow>, total: QueryTotal, unknownCount: number, observedAt: Timestamp | null, completeness: ResultCompleteness, nextCursor: string | null, };
export type QueryPageResponse = FolderQueryPage | SearchQueryPage;
export type BrowseBreadcrumb = { item: ItemRef,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayName: BrowseValue<string>,
/**
 * The root item of a confirmed macOS startup volume group. GUI channel only.
 */
isStartupVolume?: boolean, };
export type BrowseRootKind = "virtual_root" | "scope_root" | "folder";
export type BrowseContext = { origin: ResultOrigin,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, location: ItemRef | null, breadcrumbs: Array<BrowseBreadcrumb>, rootKind: BrowseRootKind, currentCorrespondence: boolean, sortStrategies: Array<QueryStrategy>, predicateStrategies: Array<QueryStrategy>,
/**
 * Entries directly under the opened folder that an exclusion rule left out.
 *
 * Exact zero means confirmed none. A lower bound means the folder's listing was
 * cut short. Names an exclusion rule could not decide are not counted. Omitted
 * when the source does not supply this fact, when it cannot be decided yet (a
 * result of a scan in progress where it is not known that the folder's listing
 * has ended), and for the virtual top level.
 */
excludedChildren?: QueryTotal, };
export type BrowseOpenResponse = { "kind": "opened", requestSeq: number, origin: ResultOrigin,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, location: ItemRef | null, breadcrumbs: Array<BrowseBreadcrumb>, rootKind: BrowseRootKind, currentCorrespondence: boolean, page: FolderQueryPage, sortStrategies: Array<QueryStrategy>, predicateStrategies: Array<QueryStrategy>,
/**
 * Entries directly under the opened folder that an exclusion rule left out;
 * see `BrowseContext::excluded_children`.
 */
excludedChildren?: QueryTotal, } | { "kind": "job", requestSeq: number, jobRef: Id,
/**
 * Source result version pinned when the request was admitted.
 */
resultVersion: Id, };
export type BrowseDetailRequest = { resultVersion: Id, item: ItemRef, };
export type BrowseDetail = { item: ItemRef,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayPath: BrowseValue<string>,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayName: BrowseValue<string>, kind: BrowseValue<ResultItemKind>, logicalSize: BrowseValue<SizeValue>, allocatedSize: BrowseValue<SizeValue>, modifiedAt: BrowseValue<Timestamp>, createdAt: BrowseValue<Timestamp>, observedAt: BrowseValue<Timestamp>, origin: BrowseOrigin,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, completeness: ResultCompleteness, currentCorrespondence: boolean, observationOnly: boolean,
/**
 * The scan state of a folder, the same value its row carries in a folder page
 * (F02 #347): whether it or a place inside it was not read, so a lower bound of 0
 * can be told from a lower bound where every counted item is empty. Omitted for a
 * file and for a sample. `null` is rejected.
 */
folderFlags?: FolderFlags, };
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
export type QueryRunRequest = {
/**
 * A query region opened with kind `search`.
 */
regionRef: Id,
/**
 * Strictly increasing per region; an equal or lower value is `command.body_invalid`.
 */
requestSeq: number,
/**
 * Absent selects the current analysis, as `browse.open`.
 */
sourceRef?: BrowseSourceRef,
/**
 * The version the caller shows; another current version is `result.requery_required`.
 */
resultVersion?: Id,
/**
 * A folder of that version; candidates are the items below it at any depth.
 * Absent: below every scope root. The folder itself is not a candidate.
 */
scope?: ItemRef, condition: QueryCondition,
/**
 * Absent: `sort.path` ascending. Ties follow the path, then the item number.
 */
sortKeys?: Array<BrowseSortKey>,
/**
 * Default 200, 1 to 1000; anything else is `cursor.limit_exceeded`.
 */
pageSize?: number, };
export type QueryRunResponse = { "kind": "opened", requestSeq: number, resultVersion: Id, origin: ResultOrigin,
/**
 * Data source of the pinned version; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource,
/**
 * The requested folder, or null for every scope root.
 */
scope: ItemRef | null, page: SearchQueryPage, } | { "kind": "job", requestSeq: number, jobRef: Id,
/**
 * Source result version pinned when the request was admitted.
 */
resultVersion: Id, };
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
export type SpaceDetail = { item: ItemRef, logical: BrowseValue<SpaceDetailSize>, allocated: BrowseValue<SpaceDetailSize>, uniquePhysical: BrowseValue<SpaceDetailSize>, exclusive: BrowseValue<SpaceDetailSize>, shared: BrowseValue<SpaceDetailSize>, linkGroup: BrowseValue<SpaceLinkGroup>, placeholderLocalState?: BrowseValue<SpacePlaceholderState>, auxiliary: BrowseValue<SpaceAuxiliary>, exclusiveDecision: SpaceExclusiveDecision, observedAt: BrowseValue<Timestamp>, capabilities: BrowseValue<Array<SpaceDetailCapability>>,
/**
 * Data source of the pinned version; absent means a sample (F32 FR9.HD1).
 */
dataSource?: DataSource,
/**
 * The scan state of a folder, the same value its row carries in a folder page
 * (F02 #347). Omitted for a file and for a sample. `null` is rejected.
 */
folderFlags?: FolderFlags, };
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
export type SnapshotSaveRequest = {
/**
 * Result version to capture.
 */
resultVersionId: Id,
/**
 * Whether the new snapshot should become the baseline.
 */
pin: boolean, };
export type SnapshotSaveResponse = SnapshotSaveAccepted | SnapshotSaveExistingSnapshot | SnapshotSaveExistingJob;
export type SnapshotSaveAccepted = {
/**
 * Fixed outcome tag.
 */
outcome: SnapshotSaveAcceptedOutcome,
/**
 * Admitted background job.
 */
jobId: Id, };
export type SnapshotSaveAcceptedOutcome = "accepted";
export type SnapshotPinApplied = false;
export type SnapshotSaveExistingSnapshot = {
/**
 * Fixed outcome tag.
 */
outcome: SnapshotSaveExistingOutcome,
/**
 * Existing snapshot identity.
 */
snapshotId: Id,
/**
 * Existing snapshots are not pinned by a save request.
 */
pinApplied: SnapshotPinApplied, };
export type SnapshotSaveExistingJob = {
/**
 * Fixed outcome tag.
 */
outcome: SnapshotSaveExistingOutcome,
/**
 * Active save job identity.
 */
jobId: Id,
/**
 * A duplicate save does not alter pin state.
 */
pinApplied: SnapshotPinApplied, };
export type SnapshotSaveExistingOutcome = "existing";
export type SnapshotListRequest = {
/**
 * Opaque continuation token returned by the previous page.
 */
cursor?: string,
/**
 * Requested page size; zero and negative values are invalid.
 */
limit?: number, };
export type SnapshotListResponse = {
/**
 * Snapshot-index sequence read with these rows.
 */
listSeq: number,
/**
 * Active snapshot summaries in observation-time order.
 */
items: Array<SnapshotSummary>,
/**
 * Opaque continuation token, omitted when this is the final page.
 */
nextCursor?: string, };
export type SnapshotSummary = {
/**
 * Snapshot identity.
 */
snapshotId: Id,
/**
 * Observation time, when known.
 */
observedAt?: Timestamp,
/**
 * Lowercase hexadecimal scope marker; omitted when the marker is empty.
 */
scopeLabel?: string,
/**
 * Stored condition-summary JSON text.
 */
conditionSummary: string,
/**
 * Completeness recorded in the snapshot, when known.
 */
completeness?: SnapshotListCompleteness,
/**
 * Snapshot format version.
 */
formatVersion: number,
/**
 * Sealed snapshot body size in bytes.
 */
bodySize: number,
/**
 * Whether this snapshot is the current baseline.
 */
pinned: boolean,
/**
 * Whether this snapshot was captured locally or imported.
 */
sourceKind: SnapshotListSourceKind,
/**
 * Volume identities stored with the snapshot, for GUI use only.
 */
volumes: Array<SnapshotListVolume>, };
export type SnapshotListCompleteness = "complete" | "partial" | "missing";
export type SnapshotListSourceKind = "local" | "imported";
export type SnapshotListVolume = {
/**
 * Zero-based position in the snapshot.
 */
volumeIndex: number,
/**
 * Lowercase hexadecimal observed identity, when known.
 */
observedVolumeId?: string, };
export type SnapshotChange = "saved" | "imported" | "pinned" | "unpinned" | "deleted" | "swept";
export type SnapshotChangedBody = {
/**
 * Committed snapshot-index list sequence.
 */
listSeq: number,
/**
 * Mutation represented by this event.
 */
change: SnapshotChange,
/**
 * Snapshot identities affected by the mutation.
 */
snapshotIds: Array<Id>,
/**
 * Time at which the mutation was observed, when available.
 */
observedAt?: Timestamp | null, };
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
export type GroupMemberRow = { item: ItemRef,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayName: BrowseValue<string>,
/**
 * Observed path as display text, never a group name; never reopen or compare by it.
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
dataSource?: DataSource,
/**
 * Human-readable display text; never reopen or compare a path from it.
 */
displayPath: BrowseValue<string>, observedAt?: Timestamp, rules: Array<GroupRuleDecision>, };
export type GroupTotalsRequest = { classificationRef: Id, groupIds: Array<Id>, };
export type GroupTotal = { sizes: Array<SpaceTotalSize>, completeness: ResultCompleteness, };
export type GroupTotalRow = { groupId: Id, total: GroupTotal, };
export type GroupRatio = { "state": "value", basis: SizeBasis, numerator: ByteSize, denominator: ByteSize, } | { "state": "undefined", basis: SizeBasis, reasonKey: string, };
export type GroupTotals = { classificationRef: Id, resultVersion: Id,
/**
 * Data source of the pinned version, always set by the producer; absent means a sample (F32 FR2.HD2).
 */
dataSource?: DataSource, groups: Array<GroupTotalRow>, union: GroupTotal, overlap: GroupTotal, overlapCount: number, unclassified: GroupTotal, undecidable: GroupTotal, scope: GroupTotal, ratios: Array<GroupRatio>, sizeRule: QueryStrategy, };
export type ChangeAction = "trash" | "move" | "delete";
export type PlanSourceKind = "manual" | "goal_alternative" | "duplicate_keep" | "scenario";
export type PlanOriginChannel = "gui" | "cli" | "mcp";
export type PlanReadiness = "experience_plan" | "source_not_current" | "needs_confirmation" | "no_executable_items" | "approval_possible";
export type PlanSection = "executable" | "held" | "needs_confirmation" | "absorbed";
export type PlanItemDisposition = "included" | "held" | "rejected";
export type PlanHash = string;
export type PlanSourceCount = { source: PlanSourceKind, count: number, };
export type PlanDraftSummary = { itemCount: number, bySource: Array<PlanSourceCount>, conversionHeldCount: number, };
export type PlanDraftOpenRequest = { source: PlanSourceKind, itemRefs: Array<ItemRef>,
/**
 * Omitted or null means no alternative reference; the source determines
 * whether `itemRefs` or this reference is required.
 */
alternativeRef?: Id, };
export type PlanDraftOpenResponse = { draftRef: Id, editSeq: number, summary: PlanDraftSummary, };
export type PlanDraftEdit = { "kind": "add_item", itemRef: ItemRef, } | { "kind": "remove_item", itemRef: ItemRef, } | { "kind": "set_action", itemRef: ItemRef, action: ChangeAction, } | { "kind": "set_destination", itemRef: ItemRef,
/**
 * Null or omitted clears the destination.
 */
destination?: OsPath, } | { "kind": "set_retention_condition", itemRef: ItemRef,
/**
 * Null or omitted clears the retention condition reference.
 */
retentionConditionRef?: Id, } | { "kind": "set_overwrite", itemRef: ItemRef, overwrite: boolean, };
export type PlanDraftEditRequest = { draftRef: Id, editSeq: number, edit: PlanDraftEdit, };
export type PlanDraftEditResponse = { draftRef: Id, editSeq: number, summary: PlanDraftSummary, };
export type PlanDraftCancelRequest = { draftRef: Id, };
export type PlanDraftCancelResponse = { discarded: boolean, buildCancelled: boolean, };
export type PlanBuildRequest = { draftRef: Id, editSeq: number, };
export type PlanBuildResponse = { jobRef: Id, };
export type PlanGetRequest = { planVersionId: Id, section: PlanSection,
/**
 * Omitted or null means no protection-class filter.
 */
protectionFilter?: string,
/**
 * Omitted or null starts at the first page.
 */
cursor?: string, };
export type PlanVersionHead = { planVersionId: Id, lineageId: Id,
/**
 * Null or omission means this is the first version in the lineage.
 */
parentPlanVersionId?: Id, planHash: PlanHash, hashFormat: string, readiness: PlanReadiness, protectionRulesetVersion: string, protectionClassTableVersion: string,
/**
 * The feature DLD defines this summary's meaning; nested fields remain
 * opaque to consumers of the common plan head.
 */
effectSummary: unknown,
/**
 * The feature DLD defines this summary's meaning; nested fields remain
 * opaque to consumers of the common plan head.
 */
restorationLimitSummary: unknown, originChannel: PlanOriginChannel, fixedAt: Timestamp, changeSummary: string, };
export type PlanVersionItem = { itemNo: number, itemRef: ItemRef, action: ChangeAction,
/**
 * Omitted or null means the action has no destination.
 */
destination?: OsPath,
/**
 * Omitted or null means no retention condition is attached.
 */
retentionConditionRef?: Id, disposition: PlanItemDisposition,
/**
 * Whether an existing destination item is selected for overwrite.
 */
overwrite: boolean,
/**
 * Expected identity and baseline facts captured when the version was fixed.
 */
expectedIdentity: unknown,
/**
 * Protection evaluation details captured with this immutable item.
 */
protectionSummary: unknown,
/**
 * Omitted or null means the item has no single review reason.
 */
reasonKey?: string, selectionBasis: Array<string>,
/**
 * Omitted or null means this item does not represent absorbed items.
 */
absorbedBy?: ItemRef, };
export type PlanGetResponse = { head: PlanVersionHead, items: Array<PlanVersionItem>,
/**
 * Omitted or null means there is no next page.
 */
nextCursor?: string, };
export type PlanItemReasonRequest = { planVersionId: Id, itemNo: number, };
export type PlanItemReasonResponse = { itemRef: ItemRef, selectionBasis: Array<string>, restrictions: Array<string>, disposition: PlanItemDisposition,
/**
 * Omitted or null means no review reason applies.
 */
reasonKey?: string, };
export type PlanHistoryRequest = { lineageId: Id, };
export type PlanHistoryEntry = { planVersionId: Id, fixedAt: Timestamp, changeSummary: string, };
export type PlanHistoryResponse = { versions: Array<PlanHistoryEntry>, };
export type PlanVersionFixed = { planVersionId: Id, lineageId: Id,
/**
 * Omitted or null means the version did not replace another version.
 */
replacedPlanVersionId?: Id, planHash: PlanHash, readiness: PlanReadiness, includedCount: number, heldCount: number, needsConfirmationCount: number, jobRef: Id, fixedAt: Timestamp, };
export type RequestKind = "plan" | "restore" | "move_undo";
export type ConfirmationKind = "release_default_protection" | "overwrite";
export type ApprovalState = "in_review" | "granted" | "rejected" | "withdrawn" | "in_use" | "spent" | "invalidated";
export type ApprovalEventState = "granted" | "consumed" | "revoked" | "rejected";
export type ApprovalProtectionResult = { itemRef: ItemRef, disposition: PlanItemDisposition,
/**
 * Omitted or null means no protection reason was recorded.
 */
reasonKey?: string, };
export type ApprovalProtectionSummary = { rulesetVersion: string, classTableVersion: string, unlockedClasses: Array<string>, unlockedItemCount: number, unlockedBytes: ByteSize, requiredBasicProtectionAck: boolean, };
export type ApprovalOverwriteSummary = { overwriteItemCount: number, displacedBytes: ByteSize, requiredOverwriteAck: boolean, };
export type ApprovalReviewSummary = { targetCount: number, targetSize: ByteSize, visibleSelectionCount: number, hiddenSelectionCount: number, hiddenDescendantCount: number, actions: Array<ChangeAction>,
/**
 * Omitted or null means the request has no destination.
 */
destination?: OsPath, retentionConditions: Array<Id>,
/**
 * Nested fields are owned by the corresponding feature summaries.
 */
effectSummary: unknown, protectionResults: Array<ApprovalProtectionResult>,
/**
 * Nested fields are owned by the corresponding feature summaries.
 */
restorationLimitSummary: unknown,
/**
 * Omitted or null means no basic-protection release applies.
 */
protectionSummary?: ApprovalProtectionSummary,
/**
 * Omitted or null means no overwrite confirmation applies.
 */
overwriteSummary?: ApprovalOverwriteSummary, };
export type ApprovalReviewRequest = { requestVersionId: Id, requestKind: RequestKind, };
export type ApprovalReviewResponse = { requestVersionId: Id, summary: ApprovalReviewSummary, planHash: PlanHash, approvalPossible: boolean,
/**
 * Omitted or null means there are no known blocking reasons.
 */
unavailableReasonKeys?: Array<string>,
/**
 * Omitted or null means the registry has no current state for this version.
 */
currentApprovalState?: ApprovalState, };
export type ApprovalConfirmationProofs = {
/**
 * Omitted or null means false; required only for permanent-delete requests.
 */
permanentDeleteSelected?: boolean,
/**
 * Omitted or null means the target count was not confirmed.
 */
confirmedTargetCount?: number,
/**
 * Omitted or null means the target size was not confirmed.
 */
confirmedTargetSize?: ByteSize,
/**
 * Omitted or null means false; true is required only for permanent-delete requests.
 */
irreversibilityConfirmed?: boolean,
/**
 * Omitted or null means false; true is required when basic protection is released.
 */
basicProtectionUnlockConfirmed?: boolean,
/**
 * Omitted or null means false; true is required when overwrite is selected.
 */
overwriteConfirmed?: boolean,
/**
 * Omitted or null means no overwrite count was confirmed.
 */
confirmedOverwriteItemCount?: number,
/**
 * Omitted or null means no displaced size was confirmed.
 */
confirmedDisplacedBytes?: ByteSize, };
export type ApprovalGrantRequest = { requestVersionId: Id, presentedPlanHash: PlanHash,
/**
 * Omitted or null means no additional proof was submitted.
 */
confirmations?: ApprovalConfirmationProofs, };
export type ApprovalGrantResponse = { state: ApprovalState, };
export type ApprovalRevokeRequest = { requestVersionId: Id, };
export type ApprovalRevokeResponse = { state: ApprovalState, };
export type ApprovalStateChanged = { requestVersionId: Id, state: ApprovalEventState,
/**
 * Omitted or null means no state-change reason is attached.
 */
reasonKey?: string, sequence: number, occurredAt: Timestamp, };
export type ChangeExecuteRequest = { requestVersionId: Id, };
export type ChangeExecuteResponse = { jobRef: Id, approvalState: ApprovalState, };
export type ChangeItemState = "completed" | "skipped" | "failed" | "unprocessed";
export type ChangeItemResultFilter = "completed" | "failed" | "outcome_unknown" | "skipped" | "held" | "unprocessed";
export type ChangeItemResultsRequest = { jobRef: Id,
/**
 * Omitted or null means all recorded result groups.
 */
resultFilter?: ChangeItemResultFilter,
/**
 * Omitted or null starts at the first result.
 */
cursor?: string, };
export type ChangeItemCounts = { completed: number,
/**
 * The shared job summary includes this category; `change.trash` leaves it zero.
 */
overwritten: number, skipped: number, failed: number, unprocessed: number, };
export type ChangeItemResult = { itemRef: ItemRef,
/**
 * F11 `change.trash` uses `state`; it does not emit an `overwritten` item state.
 */
state: ChangeItemState,
/**
 * Omitted or null means no item-specific reason was recorded.
 */
reasonKey?: string,
/**
 * Omitted or null means the size was not observed.
 */
observedSize?: ByteSize, reviewNeeded: boolean, held: boolean, outcomeUnknown: boolean, undoAvailable: boolean, };
export type ChangeItemResultsResponse = { jobRef: Id, jobState: JobState, counts: ChangeItemCounts, itemRecording: HistoryItemRecording, items: Array<ChangeItemResult>,
/**
 * Omitted or null means there is no next page.
 */
nextCursor?: string, };
export const SIZE_UNIT_RULES = {"decimalPlaces":1,"iec":{"base":1024,"symbols":["B","KiB","MiB","GiB","TiB","PiB"]},"revision":1,"si":{"base":1000,"symbols":["B","kB","MB","GB","TB","PB"]}} as const;
