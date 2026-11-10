//! Own snapshot format, comparison and catalog policy.
pub mod envelope;
mod record_framing;
pub mod records;
pub mod retention;
pub mod zstd_frame;
pub mod zstd_stream;
