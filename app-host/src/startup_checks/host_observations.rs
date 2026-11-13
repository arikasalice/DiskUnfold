//! Startup rows the host can observe before the window exists.
use du_core_contracts::{
    product_support::EmbeddedProductInfo, startup::StartupCapabilitiesPort,
    startup_ipc::StartupCheckResult,
};
use du_platform_os::support_environment::SystemSupportEnvironment;

use super::{
    capability_observation, report_pre_window_error, resource_integrity_observation,
    support_matrix_observation, webview_observation,
};

/// Observes startup rows 1, 2, 5 and 6 in table order.
///
/// A damaged embedded asset set or a webview known to be missing stops startup:
/// the failure is shown through the native pre-window dialog and returned.
pub(crate) fn host_observations(
    capabilities: &dyn StartupCapabilitiesPort,
    manifest: &str,
    assets: &[u8],
    assets_sha256: &str,
    product: &EmbeddedProductInfo,
) -> Result<Vec<StartupCheckResult>, std::io::Error> {
    let integrity = resource_integrity_observation(manifest, assets, assets_sha256)
        .map_err(|error| report_pre_window_error(capabilities, error))?;
    let webview = webview_observation(capabilities)
        .map_err(|error| report_pre_window_error(capabilities, error))?;
    let environment = SystemSupportEnvironment;
    Ok(vec![
        integrity,
        webview,
        support_matrix_observation(product, &environment),
        capability_observation(&environment),
    ])
}
