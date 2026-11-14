//! Startup language selection from the macOS display-language list.

use super::*;

/// macOS reports an ordered list such as `["ko-KR", "en-US"]`; the shared decider picks Korean.
#[test]
fn macos_style_language_list_selects_korean_when_nothing_is_stored()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = i18n::embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let (decision, unavailable) = select_with(
        &registry,
        None,
        || Ok(None),
        || Some(vec!["ko-KR".into(), "en-US".into()]),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(decision.language, "ko");
    assert_eq!(decision.source, locale::Source::Os);
    assert!(!unavailable);
    Ok(())
}
