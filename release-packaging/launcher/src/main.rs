#![cfg_attr(all(windows, feature = "gui"), windows_subsystem = "windows")]

use std::process::ExitCode;

const PRODUCT_INFO_JSON: &str = include_str!(env!("DU_PRODUCT_INFO_PATH"));

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(arg) => {
            du_cli::run_with_product_info_json(std::iter::once(arg).chain(args), PRODUCT_INFO_JSON)
        }
        None => launch_gui(),
    }
}

#[cfg(feature = "gui")]
fn launch_gui() -> ExitCode {
    match du_app_host::run_with_product_info_json(PRODUCT_INFO_JSON) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("DiskUnfold: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "gui"))]
fn launch_gui() -> ExitCode {
    eprintln!("DiskUnfold: this development build has no GUI");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::PRODUCT_INFO_JSON;

    #[test]
    fn developer_product_metadata_does_not_claim_a_source_commit() {
        assert!(PRODUCT_INFO_JSON.contains("\"buildKind\": \"developer\""));
        assert!(PRODUCT_INFO_JSON.contains("\"commit\": null"));
        assert!(PRODUCT_INFO_JSON.contains("\"sourceCommit\": null"));
    }
}
