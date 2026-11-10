//! Fully specified reusable fields of the F06 viz.render wire contract.
//!
//! The complete request and response models depend on the published architecture
//! DLD; these types do not imply that either body is complete.
use crate::{Id, Timestamp, result::ResultCompleteness};
use serde::{Deserialize, Deserializer, Serialize, de::Error};

/// A fallback table is pinned to one model and its original result revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizFallback {
    pub model_ref: String,
    pub result_revision: Id,
}

impl<'de> Deserialize<'de> for VizFallback {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            model_ref: String,
            result_revision: Id,
        }
        let wire = Wire::deserialize(deserializer)?;
        if wire.model_ref.is_empty() {
            return Err(D::Error::custom("empty visualization model reference"));
        }
        Ok(Self {
            model_ref: wire.model_ref,
            result_revision: wire.result_revision,
        })
    }
}

/// Published tree color choices; other formats normalize these fields away.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum VizColorMode {
    ParentFolder,
    Classification,
    Monochrome,
}

/// Published tree label choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum VizLabelMode {
    NameSize,
    NameOnly,
}

/// The specified non-model responses from `viz.render`.
///
/// This is intentionally not the complete response union: the physical model
/// body is not defined by these types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum VizRenderNonModelResponse {
    /// A computation remains in progress for the admitted result revision.
    Job {
        request_seq: u32,
        result_revision: Id,
        observed_at: Option<Timestamp>,
        completeness: ResultCompleteness,
        job_ref: Id,
    },
    /// The requested format is unavailable, optionally with its same-revision table.
    Unavailable {
        request_seq: u32,
        result_revision: Id,
        observed_at: Option<Timestamp>,
        completeness: ResultCompleteness,
        reason_key: String,
        available_formats: Vec<String>,
        fallback: Option<VizFallback>,
    },
}

impl VizRenderNonModelResponse {
    /// Checks the fallback's revision before emitting a constructed response.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Self::Unavailable {
            result_revision,
            fallback: Some(fallback),
            ..
        } = self
            && &fallback.result_revision != result_revision
        {
            return Err("fallback revision differs from render result");
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for VizRenderNonModelResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(
            tag = "kind",
            rename_all = "snake_case",
            rename_all_fields = "camelCase",
            deny_unknown_fields
        )]
        enum Wire {
            Job {
                request_seq: u32,
                result_revision: Id,
                #[serde(deserialize_with = "required_nullable")]
                observed_at: Option<Timestamp>,
                completeness: ResultCompleteness,
                job_ref: Id,
            },
            Unavailable {
                request_seq: u32,
                result_revision: Id,
                #[serde(deserialize_with = "required_nullable")]
                observed_at: Option<Timestamp>,
                completeness: ResultCompleteness,
                reason_key: String,
                available_formats: Vec<String>,
                #[serde(deserialize_with = "required_nullable")]
                fallback: Option<VizFallback>,
            },
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Job {
                request_seq,
                result_revision,
                observed_at,
                completeness,
                job_ref,
            } => Self::Job {
                request_seq,
                result_revision,
                observed_at,
                completeness,
                job_ref,
            },
            Wire::Unavailable {
                request_seq,
                result_revision,
                observed_at,
                completeness,
                reason_key,
                available_formats,
                fallback,
            } => {
                if fallback
                    .as_ref()
                    .is_some_and(|value| value.result_revision != result_revision)
                {
                    return Err(D::Error::custom(
                        "fallback revision differs from render result",
                    ));
                }
                Self::Unavailable {
                    request_seq,
                    result_revision,
                    observed_at,
                    completeness,
                    reason_key,
                    available_formats,
                    fallback,
                }
            }
        })
    }
}

fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
