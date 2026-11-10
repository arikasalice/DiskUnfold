//! Fully specified reusable fields of the F06 viz.render wire contract.
//!
//! The complete request and response models depend on the published architecture
//! DLD; these types do not imply that either body is complete.
use crate::Id;
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
