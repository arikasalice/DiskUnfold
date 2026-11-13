//! Pure F18 change coalescing and internal handoff values.
//!
//! The caller supplies verified, normalized paths and serializes access per
//! watch unit. No filesystem access, event publication, or wire format occurs here.
mod buffer;
mod model;
mod policy;
pub use model::{
    Batch, Change, ChangeKind, Config, Context, Cursor, Error, Gap, GapReason, Key, Signal, Trust,
};
pub use policy::Coalescer;
#[cfg(test)]
mod tests;
