//! Reads the Linux session's message locale without mutating process state.

use std::ffi::OsString;

/// The message locale inherited from the desktop or invoking shell.
pub fn preferred_ui_languages() -> Option<Vec<String>> {
    preferred_with(|key| std::env::var_os(key))
}

fn preferred_with(mut read: impl FnMut(&str) -> Option<OsString>) -> Option<Vec<String>> {
    let value = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| read(key).filter(|value| !value.is_empty()))?;
    let tag = value.to_str()?.split(['.', '@']).next()?;
    (!tag.is_empty()).then(|| vec![tag.to_owned()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_locale_prefers_nonempty_overrides_and_strips_encoding() {
        let first = preferred_with(|key| match key {
            "LC_ALL" => Some("en_US.UTF-8".into()),
            "LC_MESSAGES" => Some("ko_KR.UTF-8".into()),
            _ => None,
        });
        let second = preferred_with(|key| match key {
            "LC_ALL" => Some("".into()),
            "LC_MESSAGES" => Some("ko_KR.UTF-8@calendar".into()),
            "LANG" => Some("en_US.UTF-8".into()),
            _ => None,
        });
        assert_eq!(first, Some(vec!["en_US".to_owned()]));
        assert_eq!(second, Some(vec!["ko_KR".to_owned()]));
        assert_eq!(preferred_with(|_| None), None);
    }
}
