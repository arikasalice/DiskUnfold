//! Verification of SLSA v1 provenance using GitHub CLI as the cryptographic verifier.
mod github_cli;
mod staged_file;
mod validation;

pub use validation::{
    GITHUB_ACTIONS_WORKFLOW_BUILD_TYPE, IN_TOTO_STATEMENT_V1, ReleaseProvenanceError,
    ReleaseProvenanceExpectation, SLSA_PROVENANCE_V1, VerifiedReleaseProvenance,
    verify_release_provenance,
};
