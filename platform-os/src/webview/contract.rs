//! Validate native asset and message requests before invoking app callbacks.
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

#[derive(Clone, Copy)]
pub struct WebApp {
    pub asset: fn(&str) -> Option<Asset>,
    pub dispatch: fn(&str) -> Option<String>,
    pub icon: &'static [u8],
}

pub(super) fn entry() -> String {
    format!("{ORIGIN}/index.html")
}

pub(super) fn resource(app: WebApp, method: &str, url: &str) -> Option<Asset> {
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
    (app.asset)(path)
}

pub(super) fn message(app: WebApp, from_main_frame: bool, url: &str, data: &str) -> Option<String> {
    if !from_main_frame || url != entry() || data.len() > MAX_MESSAGE {
        return None;
    }
    (app.dispatch)(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: WebApp = WebApp {
        asset: |path| {
            (path == "/index.html").then_some(Asset {
                mime: "text/html",
                bytes: b"ok",
            })
        },
        dispatch: |data| Some(data.to_owned()),
        icon: &[],
    };

    #[test]
    fn resources_are_exact_and_local() {
        assert!(resource(APP, "GET", &entry()).is_some());
        for path in [
            "/../index.html",
            "/%69ndex.html",
            "/index.html?x",
            ".evil/index.html",
            "//index.html",
        ] {
            assert!(resource(APP, "GET", &format!("{ORIGIN}{path}")).is_none());
        }
        assert!(resource(APP, "POST", &entry()).is_none());
    }

    #[test]
    fn ipc_requires_the_trusted_top_level_document() {
        assert_eq!(message(APP, true, &entry(), "ok").as_deref(), Some("ok"));
        assert!(message(APP, false, &entry(), "no").is_none());
        assert!(message(APP, true, "https://example.invalid/", "no").is_none());
        assert!(message(APP, true, &entry(), &"x".repeat(MAX_MESSAGE + 1)).is_none());
    }
}
