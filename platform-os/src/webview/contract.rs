//! Validate native asset and message requests before invoking app callbacks.
use std::sync::Arc;

pub(super) const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; font-src 'self'; connect-src 'none'; frame-src 'none'; frame-ancestors 'none'; form-action 'none'; base-uri 'none'";
const MAX_MESSAGE: usize = 1024 * 1024;
#[cfg(target_os = "windows")]
pub(super) const ORIGIN: &str = "https://diskunfold.invalid";
#[cfg(not(target_os = "windows"))]
pub(super) const ORIGIN: &str = "diskunfold://app";

#[derive(Clone, Copy)]
pub struct Asset {
    pub mime: &'static str,
    pub bytes: &'static [u8],
}

pub type Dispatch = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Non-blocking source of serialized application event envelopes.
#[cfg(target_os = "windows")]
pub type Events = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Windows dispatch lane selected by the application after parsing the request.
#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchLane {
    Ordered,
    Volume,
    QueryControl,
    /// FIFO delivery for pause, resume and cancellation of admitted jobs.
    JobControl,
}

pub struct WebApp {
    pub asset: fn(&str) -> Option<Asset>,
    pub dispatch: Dispatch,
    /// Selects a bounded Windows queue; the dispatch callback still validates every request.
    #[cfg(target_os = "windows")]
    pub route: fn(&str) -> DispatchLane,
    pub icon: &'static [u8],
}

pub(super) fn entry() -> String {
    format!("{ORIGIN}/index.html")
}

pub(super) fn resource(asset: fn(&str) -> Option<Asset>, method: &str, url: &str) -> Option<Asset> {
    let path = url.strip_prefix(ORIGIN)?;
    if method != "GET"
        || !path.starts_with('/')
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b))
        || path[1..]
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    asset(path)
}

pub(super) fn message(
    dispatch: &Dispatch,
    from_main_frame: bool,
    url: &str,
    data: &str,
) -> Option<String> {
    if !from_main_frame || !trusted_message(url, data) {
        return None;
    }
    dispatch(data)
}

pub(super) fn trusted_message(url: &str, data: &str) -> bool {
    url == entry() && data.len() <= MAX_MESSAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(path: &str) -> Option<Asset> {
        (path == "/index.html").then_some(Asset {
            mime: "text/html",
            bytes: b"ok",
        })
    }

    #[test]
    fn resources_are_exact_and_local() {
        assert!(resource(asset, "GET", &entry()).is_some());
        for path in [
            "/../index.html",
            "/%69ndex.html",
            "/index.html?x",
            ".evil/index.html",
            "//index.html",
        ] {
            assert!(resource(asset, "GET", &format!("{ORIGIN}{path}")).is_none());
        }
        assert!(resource(asset, "POST", &entry()).is_none());
    }

    #[test]
    fn ipc_requires_the_trusted_top_level_document() {
        let dispatch: Dispatch = Arc::new(|data| Some(data.to_owned()));
        assert_eq!(
            message(&dispatch, true, &entry(), "ok").as_deref(),
            Some("ok")
        );
        assert!(message(&dispatch, false, &entry(), "no").is_none());
        assert!(message(&dispatch, true, "https://example.invalid/", "no").is_none());
        assert!(message(&dispatch, true, &entry(), &"x".repeat(MAX_MESSAGE + 1)).is_none());
    }
}
