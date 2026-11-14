//! The sample bundle compiled into the executable.
use std::sync::Arc;

use super::format_v1::SampleFormatRegistry;
use super::{BundleFailure, BundleSourcePort, ValidatedBundle, validate_bundle};

/// Format version 1 bundle built from `assets/sample/bundle.v1.json` by the
/// `build_sample_bundle` example.
static EMBEDDED_BUNDLE: &[u8] = include_bytes!("../../assets/sample/bundle.v1.bin");

/// Source of the bundle embedded in the executable. An empty asset counts as absent.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmbeddedSampleBundle;

impl EmbeddedSampleBundle {
    /// Returns the embedded bytes exactly as stored in the executable.
    pub fn bytes() -> &'static [u8] {
        EMBEDDED_BUNDLE
    }

    /// Validates the embedded bundle with the standard format registry.
    pub fn validate() -> Result<ValidatedBundle, BundleFailure> {
        validate_bundle(&Self, &SampleFormatRegistry::standard())
    }
}

impl BundleSourcePort for EmbeddedSampleBundle {
    fn read_bundle(&self) -> Option<Arc<[u8]>> {
        (!EMBEDDED_BUNDLE.is_empty()).then(|| Arc::from(EMBEDDED_BUNDLE))
    }
}
