//! The published F06 response, including an immediately available model.
use super::{model::VizModel, render::VizRenderNonModelResponse, required_nullable};
use crate::{Id, Timestamp, result::ResultCompleteness};
use serde::{Deserialize, Deserializer, Serialize, de::Error};
use std::num::NonZeroU32;

/// A model and the immutable result revision it was computed from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum VizRenderModelResponse {
    Model {
        request_seq: NonZeroU32,
        result_revision: Id,
        #[serde(deserialize_with = "required_nullable")]
        observed_at: Option<Timestamp>,
        completeness: ResultCompleteness,
        model: Box<VizModel>,
    },
}

/// The successful branches of the render call's result body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum VizRenderResponse {
    Model(VizRenderModelResponse),
    NonModel(VizRenderNonModelResponse),
}

impl VizRenderResponse {
    /// Validates a response constructed by the engine before it is sent.
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Model(VizRenderModelResponse::Model {
                result_revision,
                model,
                ..
            }) => model.validate_revision(result_revision),
            Self::NonModel(response) => {
                let sequence = match response {
                    VizRenderNonModelResponse::Job { request_seq, .. }
                    | VizRenderNonModelResponse::Unavailable { request_seq, .. } => *request_seq,
                };
                if sequence == 0 {
                    return Err("invalid visualization request sequence");
                }
                response.validate()
            }
        }
    }
}

impl<'de> Deserialize<'de> for VizRenderResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Model(VizRenderModelResponse),
            NonModel(VizRenderNonModelResponse),
        }
        let response = match Wire::deserialize(deserializer)? {
            Wire::Model(value) => Self::Model(value),
            Wire::NonModel(value) => Self::NonModel(value),
        };
        response.validate().map_err(D::Error::custom)?;
        Ok(response)
    }
}
