//! Read-only access to raw and verified protection rule versions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};

/// Hash format used by the built-in protection ruleset.
pub const PROTECTION_RULESET_HASH_FORMAT: &str = "du.protect.ruleset.v1";

/// Final F29 v1 classification IDs from the rev 89 draft.
///
/// Keep the identifiers centralized here so a future standardization change
/// requires updating only this module.
pub mod classification_id {
    /// Filesystem root or standard top-level folder.
    pub const ROOT: &str = "protect.class.root";
    /// Proven operating-system component.
    pub const OS_COMPONENT: &str = "protect.class.os_component";
    /// Component whose ownership is not established.
    pub const COMPONENT_UNCONFIRMED: &str = "protect.class.component_unconfirmed";
    /// Memory file.
    pub const MEMORY_FILE: &str = "protect.class.memory_file";
    /// Backup or snapshot.
    pub const BACKUP_SNAPSHOT: &str = "protect.class.backup_snapshot";
    /// Update residue.
    pub const UPDATE_RESIDUE: &str = "protect.class.update_residue";
    /// Guidance-only log or diagnostic data.
    pub const LOG_DIAGNOSTICS: &str = "protect.class.log_diagnostics";
    /// Directly cleanable item.
    pub const DIRECT_CLEANABLE: &str = "protect.class.direct_cleanable";
    /// Cleanable third-party system cache.
    pub const SYSTEM_CACHE_CLEANABLE: &str = "protect.class.system_cache_cleanable";
    /// Operating-system managed system cache.
    pub const SYSTEM_CACHE_GUIDANCE: &str = "protect.class.system_cache_guidance";
    /// Application-managed user data.
    pub const APP_MANAGED_USER_DATA: &str = "protect.class.app_managed_user_data";
    /// Installed application.
    pub const INSTALLED_APP: &str = "protect.class.installed_app";
    /// Trash storage.
    pub const TRASH_STORE: &str = "protect.class.trash_store";
    /// User data inside an operating-system area.
    pub const OS_AREA_USER_ITEM: &str = "protect.class.os_area_user_item";
    /// User-managed data.
    pub const USER_MANAGED: &str = "protect.class.user_managed";
    /// Memory-backed temporary item.
    pub const MEMORY_BACKED_TEMP: &str = "protect.class.memory_backed_temp";

    /// Complete classification table in UTF-8 byte sort order.
    pub const ALL: &[&str] = &[
        APP_MANAGED_USER_DATA,
        BACKUP_SNAPSHOT,
        COMPONENT_UNCONFIRMED,
        DIRECT_CLEANABLE,
        INSTALLED_APP,
        LOG_DIAGNOSTICS,
        MEMORY_BACKED_TEMP,
        MEMORY_FILE,
        OS_AREA_USER_ITEM,
        OS_COMPONENT,
        ROOT,
        SYSTEM_CACHE_CLEANABLE,
        SYSTEM_CACHE_GUIDANCE,
        TRASH_STORE,
        UPDATE_RESIDUE,
        USER_MANAGED,
    ];
}

/// Positive version number for an immutable ruleset row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RulesetVersion(NonZeroU64);

impl RulesetVersion {
    /// Creates a version only when it is positive.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the stored version number.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Positive classification table version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassTableVersion(NonZeroU32);

impl ClassTableVersion {
    /// Version one of the F29 classification table.
    pub const V1: Self = Self(NonZeroU32::MIN);

    /// Creates a version only when it is positive.
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the stored version number.
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// Positive known-location table version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KnownLocationTableVersion(NonZeroU32);

impl KnownLocationTableVersion {
    /// Version one of the F16 known-location table.
    pub const V1: Self = Self(NonZeroU32::MIN);

    /// Creates a version only when it is positive.
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the stored version number.
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// Positive rule-body format version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleFormatVersion(NonZeroU32);

impl RuleFormatVersion {
    /// Version one of the built-in protection ruleset body.
    pub const V1: Self = Self(NonZeroU32::MIN);

    /// Creates a version only when it is positive.
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the stored version number.
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// The only ruleset hash format currently supported by this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesetHashFormat {
    /// Canonical ruleset JSON hashed with BLAKE3-256.
    DuProtectRulesetV1,
}

impl RulesetHashFormat {
    /// Returns the stable storage tag.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DuProtectRulesetV1 => PROTECTION_RULESET_HASH_FORMAT,
        }
    }
}

/// Origin of an immutable protection ruleset revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesetOrigin {
    /// Built-in rules initialized by the GUI startup path.
    ProductUpdate,
    /// Rules explicitly reviewed and saved by a user.
    UserChange,
}

/// Action assigned to a classification by one ruleset snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RulesetAction {
    /// Permit the requested action.
    Allow,
    /// Deny the requested action.
    Deny,
    /// Provide guidance without a file-changing action.
    GuidanceOnly,
    /// No file-changing action is supported.
    NoChange,
    /// Never permit a file-changing action.
    Locked,
}

/// Release state for one classification in a ruleset snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassReleaseState {
    /// Protected until a later user-approved ruleset changes it.
    Protected,
    /// Released by a validated ruleset revision.
    Released,
    /// This classification cannot be released.
    NotReleasable,
}

/// Kind of an explicit user protection rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRuleKind {
    /// Always keep the matching item.
    Keep,
    /// Restrict the operations permitted for the matching item.
    AllowedOperations,
    /// Exclude the matching item from analysis.
    AnalysisExclusion,
    /// Suppress suggestions for the matching item.
    SuppressSuggestions,
}

/// User rule stored as part of one complete ruleset body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserProtectionRule {
    /// Operations permitted by an `allowed_operations` rule.
    pub allowed_operations: Vec<String>,
    /// Rule behavior.
    pub kind: UserRuleKind,
    /// Optional user memo; empty when omitted.
    pub memo: String,
    /// Native path bytes encoded as lowercase hexadecimal.
    pub path_bytes: String,
    /// Native target file identity encoded as lowercase hexadecimal.
    pub target_file_id: Option<String>,
    /// Native volume identity encoded as lowercase hexadecimal.
    pub volume_id: String,
}

/// Verified, immutable protection ruleset. Construct this only by verifying a
/// stored row; raw storage values are not authority to permit a plan item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesetSnapshot {
    /// Monotonic version from the stored row.
    pub version: RulesetVersion,
    /// Verified BLAKE3-256 hash.
    pub ruleset_hash: [u8; 32],
    /// Verified hash format.
    pub hash_format: RulesetHashFormat,
    /// Classification table version.
    pub class_table_version: ClassTableVersion,
    /// Known-location table version.
    pub known_location_table_version: KnownLocationTableVersion,
    /// Rule-body format version.
    pub rule_format_version: RuleFormatVersion,
    /// Row origin.
    pub origin: RulesetOrigin,
    /// Complete user rules; an empty vector is a valid empty rule list.
    pub user_rules: Vec<UserProtectionRule>,
    /// Action for every classification in the pinned table version.
    pub class_actions: BTreeMap<String, RulesetAction>,
    /// Release state for every classification in the pinned table version.
    pub class_release_state: BTreeMap<String, ClassReleaseState>,
}

/// One owned, unverified stored row. This is neither an integrity check nor
/// authority to make a protection decision. Consumers must validate its format,
/// rule semantics and canonical hash before using it as a policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesetRow {
    /// Monotonically increasing stored rule version.
    pub version: i64,
    /// Stored digest bytes, pending verification against the canonical body.
    pub ruleset_hash: Vec<u8>,
    /// Identifier for the stored digest format.
    pub hash_format: String,
    /// Classification table version referenced by this row.
    pub class_table_version: String,
    /// Known-location table version referenced by this row.
    pub known_location_table_version: String,
    /// Format version of the stored rule body.
    pub rule_format_version: i64,
    /// Stored origin of the version.
    pub origin: String,
    /// Stored change summary JSON.
    pub change_summary: String,
    /// Stored rule body JSON.
    pub body: String,
    /// Creation time in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
}

/// Structural corruption is distinct from inability to read the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesetReadError {
    /// A row or its storage is structurally invalid.
    Corrupt,
    /// The storage operation did not return a usable result.
    StorageUnavailable,
}

/// Each call returns one owned row from one storage snapshot. Later appends do
/// not change it, but the returned current row need not remain current.
pub trait RulesetRead {
    /// Reads the current row, or returns `None` if no version exists.
    fn read_current(&self) -> Result<Option<RulesetRow>, RulesetReadError>;
    /// Reads exactly the requested version, or returns `None` if it is absent.
    fn read_version(&self, version: i64) -> Result<Option<RulesetRow>, RulesetReadError>;
}
