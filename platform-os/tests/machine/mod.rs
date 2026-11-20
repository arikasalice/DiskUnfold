//! Checks that depend on how the machine is set up (#43).

/// Skips a check the machine cannot run. On CI, where `CI` is set, a skip
/// fails instead: a skipped check would otherwise be reported as passed, and
/// the CI runners are expected to run every one of them.
pub fn skip(reason: &str) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CI").is_some() {
        return Err(format!("not run on CI: {reason}").into());
    }
    eprintln!("skipped: {reason}");
    Ok(())
}
