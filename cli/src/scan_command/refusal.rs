//! The error record of a refused scan, and the explanation of a scan root that
//! goes through a symbolic link (#243).

use super::*;

/// Explains the first refused scan root that passes through a symbolic link.
pub(super) fn path_link_message(scan: &Scan, message: &Messages<'_>) -> Option<String> {
    for argument in scan.roots() {
        let Ok(absolute) = std::path::absolute(argument) else {
            continue;
        };
        let Some(path_link) = du_platform_os::path_link::first_symbolic_link(&absolute) else {
            continue;
        };
        let link = path_link.link.to_string_lossy();
        let target = path_link.target.to_string_lossy();
        return Some(if let Some(suggestion) = path_link.suggestion.as_deref() {
            let suggestion = suggestion.to_string_lossy();
            message.render_names(
                "cli.scan.path_through_link",
                &[
                    ("link", &link),
                    ("target", &target),
                    ("suggestion", &suggestion),
                ],
            )
        } else {
            message.render_names(
                "cli.scan.path_through_link_unresolved",
                &[("link", &link), ("target", &target)],
            )
        });
    }
    None
}

/// Writes one error record and returns the code the failure maps to; stdout stays empty.
pub(super) fn fail(stderr: &mut impl Write, message: &Messages<'_>, failure: &Failure) -> ExitCode {
    fail_with_localized(stderr, message, failure, None)
}

/// Like [`fail`], with the localized text to show instead of the default.
pub(super) fn fail_with_localized(
    stderr: &mut impl Write,
    message: &Messages<'_>,
    failure: &Failure,
    localized: Option<&str>,
) -> ExitCode {
    let exit = exit_code::exit(End::Refused(failure));
    // A usage error explains the path to fix; the others carry their key only.
    let localized = localized
        .map(str::to_owned)
        .or_else(|| (exit == ExitCode::from(2)).then(|| text(message, "cli.scan.path_unusable")));
    write_error_line(stderr, &failure.reason, &failure.code, localized.as_deref());
    exit
}
