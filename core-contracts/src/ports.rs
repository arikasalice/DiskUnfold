//! Small synchronous OS capabilities used by the test foundation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortErrorKind {
    NotFound,
    PermissionDenied,
    InUse,
    Unsupported,
    IdentityChanged,
    Busy,
    Io,
    Corrupt,
    SchemaMismatch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortError {
    pub kind: PortErrorKind,
    pub reason_key: String,
    pub os_code: Option<i32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Link,
    Placeholder,
    Volume,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: Vec<u8>,
    pub kind: EntryKind,
}
pub trait EnumeratePort: Send + Sync {
    fn enumerate(&self, path: &[u8]) -> Result<Vec<Entry>, PortError>;
}
pub trait ContentReadPort: Send + Sync {
    fn read(&self, path: &[u8], limit: usize) -> Result<Vec<u8>, PortError>;
}
pub trait Clock: Send + Sync {
    fn monotonic_millis(&self) -> u64;
}
pub trait Random: Send + Sync {
    fn fill(&self, bytes: &mut [u8]);
}
