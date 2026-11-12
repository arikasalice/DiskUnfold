//! GUI-only F06 required-data evaluation for an explicitly pinned scan folder.
use std::sync::Arc;

use du_analysis_index::{IngestSink, visualization::typed_layout::Dispatcher};
use du_core_contracts::{
    ItemRef,
    result::ResultItemKind,
    visualization::availability::{
        VizAvailabilityRequest, VizAvailabilityResponse, VizFormatAvailability,
    },
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    current_analysis::{self, CurrentAnalysis},
    error::EngineError,
    required_data::{self, Evaluation, Facts, PinnedFacts, SourceKey},
    viz_catalog,
    viz_runtime::EvidenceProducer,
};

/// Failure to prepare the opt-in availability command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationError {
    /// The dispatcher has no registered formats.
    EmptyRegistry,
    /// The supplied revision does not describe the implemented FR16 table.
    RequiredDataRevisionMismatch,
    /// The owning visualization producer did not opt into scope-only evidence.
    EvidenceNotSupported,
    /// The frozen format list is invalid.
    InvalidFormats,
}

/// Registers `viz.availability` for one frozen dispatcher and evidence producer.
///
/// This command has no product-default registration. A composition root must
/// explicitly provide the source store and trusted owner producer. The producer
/// must opt in to scope-only evidence; it remains the same owner used for render.
pub fn register(
    builder: &mut EngineBuilder,
    source: IngestSink,
    dispatcher: &Dispatcher,
    required_data_table_revision: u32,
    producer: Arc<dyn EvidenceProducer>,
) -> Result<(), RegistrationError> {
    let catalog = viz_catalog::snapshot(dispatcher).map_err(|error| match error {
        viz_catalog::Error::EmptyRegistry => RegistrationError::EmptyRegistry,
        _ => RegistrationError::InvalidFormats,
    })?;
    builder.register(prepare(
        source,
        catalog.formats,
        required_data_table_revision,
        producer,
    )?);
    Ok(())
}

pub(crate) fn prepare(
    source: IngestSink,
    formats: Vec<String>,
    required_data_table_revision: u32,
    producer: Arc<dyn EvidenceProducer>,
) -> Result<ReadAvailability, RegistrationError> {
    if required_data_table_revision != required_data::TABLE_REVISION {
        return Err(RegistrationError::RequiredDataRevisionMismatch);
    }
    let mut seen = std::collections::HashSet::with_capacity(formats.len());
    if formats.is_empty()
        || formats
            .iter()
            .any(|format| format.trim().is_empty() || !seen.insert(format.as_str()))
    {
        return Err(RegistrationError::InvalidFormats);
    }
    if !producer.supports_required_data_availability() {
        return Err(RegistrationError::EvidenceNotSupported);
    }
    Ok(ReadAvailability {
        source,
        formats,
        required_data_table_revision,
        producer,
    })
}

pub(crate) struct ReadAvailability {
    source: IngestSink,
    formats: Vec<String>,
    required_data_table_revision: u32,
    producer: Arc<dyn EvidenceProducer>,
}

impl UseCase for ReadAvailability {
    const NAME: &'static str = "viz.availability";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizAvailabilityRequest;
    type Response = VizAvailabilityResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        if request.source_ref.kind != "scan" {
            return Err(unavailable());
        }
        let source = current_analysis::admit_scan(
            &self.source,
            Some(&request.source_ref.reference),
            Some(&request.result_revision),
        )
        .map_err(source_error)?;
        validate_scope(&request.aggregate_scope_ref, &source)?;

        // The owner read is explicit about both source and folder; no render-shaped
        // request is synthesized to make this evidence appear in scope.
        let values = self
            .producer
            .required_data_facts_for_scope(&source, &request.aggregate_scope_ref)?;
        let source_key = SourceKey::from_current(&source, &request.aggregate_scope_ref)
            .map_err(|_| internal())?;
        let facts =
            Facts::bind(&source, &request.aggregate_scope_ref, values).map_err(|_| internal())?;
        let pinned = PinnedFacts::bind(&source, &request.aggregate_scope_ref, facts)
            .map_err(|_| internal())?;
        let decisions = required_data::evaluate_formats(&self.formats, &source_key, &pinned)
            .map_err(|_| internal())?;

        let formats = decisions
            .into_iter()
            .map(|decision| {
                let (available, reason_key) = match decision.evaluation {
                    Evaluation::Available => (true, None),
                    Evaluation::Unavailable(reason) => (false, Some(reason.key().to_owned())),
                };
                VizFormatAvailability {
                    format: decision.format.to_owned(),
                    available,
                    reason_key,
                }
            })
            .collect();
        let response = VizAvailabilityResponse {
            formats,
            required_data_table_revision: self.required_data_table_revision,
        };
        response.validate().map_err(|_| internal())?;
        Ok(response)
    }
}

fn validate_scope(scope: &ItemRef, source: &CurrentAnalysis) -> Result<(), EngineError> {
    if scope.result_version != *source.result_version() {
        return Err(invalid());
    }
    let detail = source.version().detail(scope).map_err(|_| invalid())?;
    if detail.item.kind != ResultItemKind::Directory {
        return Err(not_folder());
    }
    Ok(())
}

fn source_error(error: current_analysis::Error) -> EngineError {
    use current_analysis::Error::*;
    EngineError::reason(match error {
        NoSource | UnsupportedSource => "ref.unavailable",
        StaleVersion => "result.version_stale",
        Unavailable => "support.internal_error",
    })
}

fn invalid() -> EngineError {
    EngineError::reason("command.body_invalid")
}

fn not_folder() -> EngineError {
    EngineError::reason("browse.not_folder")
}

fn unavailable() -> EngineError {
    EngineError::reason("ref.unavailable")
}

fn internal() -> EngineError {
    EngineError::reason("support.internal_error")
}
