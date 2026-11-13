//! F06 availability query and its required-data result.
use super::{request::VizSourceRef, required_nullable};
use crate::{Id, ItemRef};
use serde::{Deserialize, Deserializer, Serialize, de::Error};

/// Checks whether registered visualization formats have the data they require.
///
/// The request names one source lineage, one immutable result revision and one
/// aggregate folder. The engine does not select a result on the caller's behalf.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizAvailabilityRequest {
    pub source_ref: VizSourceRef,
    pub result_revision: Id,
    pub aggregate_scope_ref: ItemRef,
}

/// Availability of one registered format, in dispatcher registration order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizFormatAvailability {
    pub format: String,
    pub available: bool,
    /// Null for an available format; required and non-null for an unavailable one.
    pub reason_key: Option<String>,
}

impl<'de> Deserialize<'de> for VizFormatAvailability {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            format: String,
            available: bool,
            #[serde(deserialize_with = "required_nullable")]
            reason_key: Option<String>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            format: wire.format,
            available: wire.available,
            reason_key: wire.reason_key,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

impl VizFormatAvailability {
    fn validate(&self) -> Result<(), &'static str> {
        if self.format.trim().is_empty() || self.available == self.reason_key.is_some() {
            return Err("invalid visualization format availability");
        }
        Ok(())
    }
}

/// Availability results evaluated against one pinned source and folder scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizAvailabilityResponse {
    pub formats: Vec<VizFormatAvailability>,
    pub required_data_table_revision: u32,
}

impl VizAvailabilityResponse {
    /// Checks the table revision and every per-format available/reason pair.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.required_data_table_revision == 0 || self.formats.is_empty() {
            return Err("invalid visualization availability response");
        }
        let mut formats = std::collections::HashSet::with_capacity(self.formats.len());
        if self
            .formats
            .iter()
            .any(|format| format.validate().is_err() || !formats.insert(format.format.as_str()))
        {
            return Err("invalid visualization availability formats");
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for VizAvailabilityResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            formats: Vec<VizFormatAvailability>,
            required_data_table_revision: u32,
        }

        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            formats: wire.formats,
            required_data_table_revision: wire.required_data_table_revision,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}
