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
    fn builds_without_a_release_request_stay_developer_builds() -> Result<(), serde_json::Error> {
        let info: serde_json::Value = serde_json::from_str(PRODUCT_INFO_JSON)?;
        if option_env!("DU_BUILD_KIND") != Some("official") {
            assert_eq!(info["build"]["buildKind"], "developer");
        }
        // Release-only fields stay unavailable unless the release pipeline supplies them.
        if option_env!("DU_BUILT_AT_UTC").is_none() {
            assert!(info["build"]["builtAtUtc"].is_null());
        }
        if option_env!("DU_APP_DB_FORMAT").is_none() {
            assert!(info["build"]["appDbFormat"].is_null());
        }
        if option_env!("DU_RELEASE_RESOURCES").is_none() {
            assert!(info["notices"]["thirdParty"].is_null());
            assert!(info["supportMatrix"].is_null());
        }
        Ok(())
    }

    #[test]
    fn the_embedded_commit_is_a_real_git_commit_when_one_is_known() -> Result<(), serde_json::Error>
    {
        let info: serde_json::Value = serde_json::from_str(PRODUCT_INFO_JSON)?;
        if let Some(commit) = info["build"]["commit"].as_str() {
            let sha = commit.strip_suffix("-dirty").unwrap_or(commit);
            assert_eq!(sha.len(), 40, "{commit}");
            assert!(sha.bytes().all(|b| b.is_ascii_hexdigit()), "{commit}");
        }
        Ok(())
    }
}
