//! Startup rows 5 and 6: support-matrix lookup and capability limits.
use du_core_contracts::{
    product_support::{
        EmbeddedProductInfo, SupportCapabilityState, SupportEnvironmentPort, SupportMatrix,
        SupportStatus,
    },
    startup_ipc::{StartupCheck, StartupCheckOutcome, StartupCheckResult},
};
use du_engine_api::support;

const OUTSIDE_VERIFIED: &str = "support.outside_verified";
const ENV_UNKNOWN: &str = "support.env_unknown";
const CAPABILITY_LIMITED: &str = "support.capability_limited";
const CAPABILITY_UNKNOWN: &str = "support.capability_unknown";
/// Capability area owned by the full-disk-access row, not by row 6.
const ACCESS_AREA: &str = "full_disk_access";

fn row(
    check: StartupCheck,
    result: StartupCheckOutcome,
    reason_key: Option<String>,
) -> StartupCheckResult {
    StartupCheckResult {
        check,
        result,
        reason_key,
    }
}

/// The embedded matrix, unless the build records a different revision than the
/// matrix carries. An inconsistent or missing matrix is evaluated as empty.
fn embedded_matrix(info: &EmbeddedProductInfo) -> SupportMatrix {
    let Some(matrix) = info.support_matrix.as_ref() else {
        return SupportMatrix::default();
    };
    match (&info.build.support_matrix_revision, &matrix.revision) {
        (Some(built), Some(embedded)) if built != embedded => SupportMatrix::default(),
        _ => matrix.clone(),
    }
}

/// Row 5: compares the observed environment identity with the embedded matrix.
///
/// A verified combination passes, a conditional one is limited, one outside the
/// matrix is outside-verified, and an unidentified environment, absent matrix or
/// invalid matrix is unverifiable. The evaluation is the same pure decision that
/// backs `app.support_info`.
pub(crate) fn support_matrix_observation(
    info: &EmbeddedProductInfo,
    environment: &dyn SupportEnvironmentPort,
) -> StartupCheckResult {
    let report = support::evaluate(&embedded_matrix(info), environment.identity(), &[]);
    let check = StartupCheck::SupportMatrix;
    match report.combination_status {
        SupportStatus::OfficiallyVerified => row(check, StartupCheckOutcome::Pass, None),
        SupportStatus::Conditional => row(check, StartupCheckOutcome::Limited, None),
        SupportStatus::NotVerified | SupportStatus::UnsupportedFound => row(
            check,
            StartupCheckOutcome::OutsideVerified,
            Some(
                report
                    .reason_key
                    .unwrap_or_else(|| OUTSIDE_VERIFIED.to_owned()),
            ),
        ),
        SupportStatus::Unknown => row(
            check,
            StartupCheckOutcome::Unverifiable,
            Some(report.reason_key.unwrap_or_else(|| ENV_UNKNOWN.to_owned())),
        ),
    }
}

/// Row 6: reduces the platform capability values to one advisory outcome.
///
/// Any unsupported or limited capability limits the product; otherwise any
/// unknown capability, or no capability evidence at all, is unverifiable. The
/// full-disk-access capability belongs to row 7 and is excluded here.
pub(crate) fn capability_observation(
    environment: &dyn SupportEnvironmentPort,
) -> StartupCheckResult {
    let check = StartupCheck::Capability;
    let capabilities: Vec<_> = environment
        .capabilities()
        .into_iter()
        .filter(|capability| capability.area != ACCESS_AREA)
        .collect();
    if capabilities.iter().any(|capability| {
        matches!(
            capability.state,
            SupportCapabilityState::Limited | SupportCapabilityState::Unsupported
        )
    }) {
        return row(
            check,
            StartupCheckOutcome::Limited,
            Some(CAPABILITY_LIMITED.to_owned()),
        );
    }
    if capabilities.is_empty()
        || capabilities.iter().any(|capability| {
            !matches!(
                capability.state,
                SupportCapabilityState::Supported | SupportCapabilityState::NotApplicable
            )
        })
    {
        return row(
            check,
            StartupCheckOutcome::Unverifiable,
            Some(CAPABILITY_UNKNOWN.to_owned()),
        );
    }
    row(check, StartupCheckOutcome::Pass, None)
}
