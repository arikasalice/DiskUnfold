//! Reads macOS's preferred UI language tags without changing system preferences.

use objc2_foundation::NSLocale;

/// Returns the ordered language tags reported by Foundation for the current user.
///
/// Foundation's list is read without changing any system preference. Empty tags are dropped.
/// Returns `None` when no non-empty tag remains, that is, when Foundation reports an empty
/// list; the lookup has no separate failure path. Callers treat `None` as "no operating-system
/// language available" and use their own fallback.
pub fn preferred_ui_languages() -> Option<Vec<String>> {
    let languages = NSLocale::preferredLanguages();
    let tags = (0..languages.count())
        .map(|index| languages.objectAtIndex(index).to_string())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    (!tags.is_empty()).then_some(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ordered_native_preferred_languages() {
        let tags = preferred_ui_languages().unwrap_or_default();
        assert!(!tags.is_empty());
        assert!(tags.iter().all(|tag| !tag.trim().is_empty()));
    }
}
