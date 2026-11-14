//! F32 data source checks for snapshot persistence and comparison
//! (FR2.HD3.app-host.DD1, IF-14).
//!
//! A sample result must not become a snapshot, move the catalog's last result,
//! or be compared with a real one. These checks decide from a [`DataSource`]
//! alone, so a caller runs them before any work or write for the request.
//! Snapshot-store has no entry point that accepts a result version yet; the
//! owner of the first one calls these functions before it starts work.

use du_core_contracts::{
    data_source::{DataSource, MixedSource},
    sample_ipc::SNAPSHOT_SAMPLE_SOURCE,
};

/// A snapshot was requested for a result that does not come from the user's storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotSourceRejected;

impl SnapshotSourceRejected {
    /// Reason key reported with `INVALID_ARGUMENT`.
    pub const REASON: &'static str = SNAPSHOT_SAMPLE_SOURCE;
}

impl std::fmt::Display for SnapshotSourceRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(Self::REASON)
    }
}

impl std::error::Error for SnapshotSourceRejected {}

/// Admits a result as snapshot input; only [`DataSource::Real`] passes.
///
/// Call this before reading the result or writing any snapshot state.
pub fn admit_snapshot_source(source: DataSource) -> Result<(), SnapshotSourceRejected> {
    if source.is_real() {
        Ok(())
    } else {
        Err(SnapshotSourceRejected)
    }
}

/// Returns the source of a comparison over `left` and `right`.
///
/// The comparison inherits the single shared source. Inputs from different
/// sources are rejected with [`MixedSource`] before any comparison state is built.
pub fn admit_comparison_sources(
    left: DataSource,
    right: DataSource,
) -> Result<DataSource, MixedSource> {
    DataSource::derive([left, right])
}

/// Returns true when a result of `source` may update the catalog's last result.
pub fn updates_catalog_last_result(source: DataSource) -> bool {
    source.is_real()
}

#[cfg(test)]
mod tests {
    use super::*;
    use DataSource::{Real, Sample};

    #[test]
    fn snapshot_input_admits_only_real_results() {
        let rows = [
            (Real, Ok(())),
            (Sample, Err(SnapshotSourceRejected)),
            (DataSource::default(), Err(SnapshotSourceRejected)),
        ];
        for (source, expected) in rows {
            assert_eq!(admit_snapshot_source(source), expected, "{source:?}");
        }
        assert_eq!(SnapshotSourceRejected::REASON, "snapshot.sample_source");
    }

    #[test]
    fn comparison_inherits_one_source_and_rejects_mixtures() {
        let rows = [
            (Real, Real, Ok(Real)),
            (Sample, Sample, Ok(Sample)),
            (Real, Sample, Err(MixedSource)),
            (Sample, Real, Err(MixedSource)),
        ];
        for (left, right, expected) in rows {
            assert_eq!(
                admit_comparison_sources(left, right),
                expected,
                "{left:?} vs {right:?}"
            );
        }
        assert_eq!(MixedSource::REASON, "sample.mixed_source");
    }

    #[test]
    fn catalog_last_result_follows_only_real_results() {
        let rows = [(Real, true), (Sample, false)];
        for (source, expected) in rows {
            assert_eq!(updates_catalog_last_result(source), expected, "{source:?}");
        }
    }
}
