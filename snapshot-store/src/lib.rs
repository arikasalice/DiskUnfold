//! Own snapshot format, comparison and catalog policy.
pub mod catalog_status;
pub mod comparison;
pub mod comparison_ranking;
pub mod envelope;
mod record_framing;
pub mod records;
pub mod retention;
pub mod source_admission;
pub mod storage_match;
pub mod trend;
pub mod zstd_frame;
pub mod zstd_stream;
