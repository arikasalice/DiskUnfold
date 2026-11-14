//! Data source carried by result, plan and job metadata (F32 FR2.HD1).
use serde::{Deserialize, Serialize};

/// Reason key for a derived result whose inputs come from different data sources.
pub const SAMPLE_MIXED_SOURCE: &str = "sample.mixed_source";

/// Where the observations behind a result came from.
///
/// An absent or unrecognized value is treated as [`DataSource::Sample`] so that
/// persistence, external exposure and approval paths fail closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    /// Observed from the user's storage.
    Real,
    /// Loaded from the embedded sample bundle.
    #[default]
    #[serde(other)]
    Sample,
}

/// Inputs to a derived result do not share one data source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixedSource;

impl MixedSource {
    /// Reason key reported with `INVALID_ARGUMENT`.
    pub const REASON: &'static str = SAMPLE_MIXED_SOURCE;
}

impl DataSource {
    /// Interprets an optional wire value; an absent value is a sample.
    pub fn or_sample(value: Option<Self>) -> Self {
        value.unwrap_or(Self::Sample)
    }

    /// Returns true only for results observed from the user's storage.
    pub fn is_real(self) -> bool {
        self == Self::Real
    }

    /// Returns the data source of a result derived from `inputs`.
    ///
    /// The derived result inherits the single shared source. Different sources are
    /// rejected before any derived state is built, and an empty input is a sample.
    pub fn derive(inputs: impl IntoIterator<Item = Self>) -> Result<Self, MixedSource> {
        let mut inputs = inputs.into_iter();
        let Some(first) = inputs.next() else {
            return Ok(Self::Sample);
        };
        if inputs.all(|source| source == first) {
            Ok(first)
        } else {
            Err(MixedSource)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DataSource, MixedSource};

    #[test]
    fn derived_results_inherit_one_source_and_reject_mixtures() {
        use DataSource::{Real, Sample};
        assert_eq!(DataSource::derive([Real, Real]), Ok(Real));
        assert_eq!(DataSource::derive([Sample]), Ok(Sample));
        assert_eq!(DataSource::derive([]), Ok(Sample));
        assert_eq!(DataSource::derive([Real, Sample]), Err(MixedSource));
        assert_eq!(DataSource::derive([Sample, Sample, Real]), Err(MixedSource));
    }

    #[test]
    fn absent_and_unknown_values_are_samples() -> Result<(), serde_json::Error> {
        assert_eq!(DataSource::or_sample(None), DataSource::Sample);
        assert_eq!(
            serde_json::from_str::<DataSource>("\"future_source\"")?,
            DataSource::Sample
        );
        assert_eq!(
            serde_json::from_str::<DataSource>("\"real\"")?,
            DataSource::Real
        );
        assert_eq!(serde_json::to_string(&DataSource::Sample)?, "\"sample\"");
        Ok(())
    }
}
