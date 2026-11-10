use super::*;
use crate::{
    Registry, RegistryError,
    settings::{SettingsError, SettingsService},
};
use du_core_contracts::settings::{
    SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsSource, SettingsStore,
    SettingsStoreError,
};

fn input<'a>(registered: &'a [&'a str], stored: Option<&'a str>, os: OsLanguage<'a>) -> Inputs<'a> {
    Inputs {
        registered,
        stored,
        os,
        korean_integrity_failed: false,
    }
}

#[test]
fn os_selection_uses_only_the_first_tag_and_capability() -> Result<(), DecisionError> {
    for tag in ["ko-KR", "ko_KR", "KO"] {
        let tags = [tag];
        let inputs = input(&["en", "ko"], None, OsLanguage::Supported(&tags));
        let decision = decide(&inputs)?;
        assert_eq!(
            (decision.language.as_str(), decision.source),
            ("ko", Source::Os)
        );
    }
    for os in [
        OsLanguage::Supported(&["fr", "ko"]),
        OsLanguage::Supported(&["unknown"]),
        OsLanguage::Partial(&[]),
        OsLanguage::Unavailable,
        OsLanguage::Supported(&["-KR"]),
    ] {
        let decision = decide(&input(&["en", "ko"], None, os))?;
        assert_eq!(
            (decision.language.as_str(), decision.source),
            ("en", Source::Baseline)
        );
    }
    assert_eq!(
        decide(&input(&["en", "ko"], None, OsLanguage::Partial(&["en-US"])))?.source,
        Source::Os
    );
    Ok(())
}

#[test]
fn stored_and_cli_precedence_and_invalid_values_are_distinct() -> Result<(), DecisionError> {
    let inputs = input(&["en", "ko"], Some("en"), OsLanguage::Supported(&["ko-KR"]));
    assert_eq!(decide(&inputs)?.source, Source::Stored);
    let from_cli = decide_cli(&inputs, Some("ko"))?;
    assert_eq!(
        (from_cli.language.as_str(), from_cli.source),
        ("ko", Source::Argument)
    );
    assert_eq!(decide_cli(&inputs, Some("KO_KR"))?, from_cli);
    assert_eq!(
        decide_cli(&inputs, Some("")),
        Err(DecisionError::UnsupportedLanguage)
    );
    assert_eq!(
        decide_cli(&inputs, Some("fr")),
        Err(DecisionError::UnsupportedLanguage)
    );
    assert_eq!(
        DecisionError::UnsupportedLanguage.reason_key(),
        "locale.unsupported"
    );
    let invalid = input(&["en", "ko"], Some("fr"), OsLanguage::Supported(&["ko"]));
    let selected = decide_cli(&invalid, None)?;
    assert_eq!(
        (
            selected.language.as_str(),
            selected.source,
            selected.stored_unavailable
        ),
        ("ko", Source::Os, true)
    );
    assert_eq!(
        decide(&input(&["ko"], None, OsLanguage::Unavailable)),
        Err(DecisionError::RegistrationFailed)
    );
    Ok(())
}

#[test]
fn corrupt_korean_forces_english_without_suppressing_unknown_cli_error() -> Result<(), DecisionError>
{
    for stored in [None, Some("ko"), Some("en")] {
        let inputs = Inputs {
            registered: &["en"],
            stored,
            os: OsLanguage::Supported(&["ko-KR"]),
            korean_integrity_failed: true,
        };
        for argument in [None, Some("en"), Some("ko"), Some("KO")] {
            let decision = decide_cli(&inputs, argument)?;
            assert_eq!(
                (decision.language.as_str(), decision.source),
                ("en", Source::Baseline)
            );
        }
        assert_eq!(decide(&inputs)?.language, "en");
        assert_eq!(
            decide_cli(&inputs, Some("fr")),
            Err(DecisionError::UnsupportedLanguage)
        );
        assert_eq!(inputs.stored, stored);
    }
    Ok(())
}

struct MemoryStore(SettingsSnapshot);
impl SettingsStore for MemoryStore {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
        Ok(self.0.clone())
    }
    fn commit(
        &mut self,
        expected: u64,
        changes: &SettingsChanges,
        _: SettingsCommit,
    ) -> Result<u64, SettingsStoreError> {
        if expected != self.0.revision {
            return Err(SettingsStoreError::Conflict);
        }
        for (key, value) in changes {
            if let Some(value) = value {
                self.0.values.insert(key.clone(), value.clone());
            } else {
                self.0.values.remove(key);
            }
        }
        self.0.revision += 1;
        Ok(self.0.revision)
    }
}

fn registry(languages: &[&str]) -> Result<Registry, String> {
    Registry::new(descriptors(languages).map_err(|e| format!("{e:?}"))?)
        .map_err(|e: RegistryError| format!("{e:?}"))
}

#[test]
fn descriptors_use_f14_types_and_absent_default_does_not_erase_explicit_english()
-> Result<(), String> {
    let rows = descriptors(&["en", "ko"]).map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows.len(), 6);
    let expected = [
        (
            "locale.display_language",
            json!(null),
            vec![json!(null), json!("en"), json!("ko")],
        ),
        (
            "locale.date_format",
            json!("lang_default"),
            vec![json!("lang_default"), json!("iso8601")],
        ),
        (
            "locale.number_format",
            json!("lang_default"),
            vec![json!("lang_default"), json!("no_grouping")],
        ),
        (
            "locale.time_zone_policy",
            json!("local"),
            vec![json!("local"), json!("utc")],
        ),
        (
            "a11y.motion",
            json!("follow_os"),
            vec![json!("follow_os"), json!("reduce")],
        ),
        (
            "a11y.app_scale",
            json!(100),
            vec![
                json!(100),
                json!(110),
                json!(125),
                json!(150),
                json!(175),
                json!(200),
            ],
        ),
    ];
    for (row, (key, default, values)) in rows.iter().zip(expected) {
        assert_eq!(row.key, key);
        assert_eq!(row.default, default);
        assert_eq!(row.kind, ValueKind::Choice(values));
    }
    for row in &rows {
        assert_eq!(row.owner, "F13");
        assert!(row.editable && crate::accepts(&row.kind, &row.default));
    }
    let snapshot = SettingsSnapshot {
        schema_version: 1,
        ..SettingsSnapshot::default()
    };
    let mut service = SettingsService::open(registry(&["en", "ko"])?, MemoryStore(snapshot))
        .map_err(|e| format!("{e:?}"))?;
    let context = SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 0,
    };
    let changes =
        SettingsChanges::from([("locale.display_language".to_owned(), Some(json!("en")))]);
    let hash = service.review(0, &changes).map_err(|e| format!("{e:?}"))?;
    service
        .apply(0, &changes, hash, context)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        service.snapshot().values.get("locale.display_language"),
        Some(&json!("en"))
    );
    let automatic =
        SettingsChanges::from([("locale.display_language".to_owned(), Some(Value::Null))]);
    let hash = service
        .review(1, &automatic)
        .map_err(|e| format!("{e:?}"))?;
    service
        .apply(1, &automatic, hash, context)
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        !service
            .snapshot()
            .values
            .contains_key("locale.display_language")
    );
    let invalid_scale = SettingsChanges::from([("a11y.app_scale".into(), Some(json!(120)))]);
    assert_eq!(
        service.review(2, &invalid_scale),
        Err(SettingsError::InvalidValue)
    );
    Ok(())
}

#[test]
fn corrupt_korean_does_not_invalidate_persisted_language() -> Result<(), String> {
    let snapshot = SettingsSnapshot {
        schema_version: 1,
        values: [("locale.display_language".into(), json!("ko"))].into(),
        ..SettingsSnapshot::default()
    };
    let service = SettingsService::open(registry(&["en", "ko"])?, MemoryStore(snapshot))
        .map_err(|e| format!("{e:?}"))?;
    let stored = service.snapshot().values["locale.display_language"].as_str();
    let decision = decide(&Inputs {
        registered: &["en"],
        stored,
        os: OsLanguage::Unavailable,
        korean_integrity_failed: true,
    })
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(decision.language, "en");
    assert_eq!(stored, Some("ko"));
    Ok(())
}

#[test]
fn gui_startup_uses_english_and_preserves_stored_language_after_korean_integrity_failure()
-> Result<(), DecisionError> {
    let stored = "ko";
    let inputs = Inputs {
        registered: &["en"],
        stored: Some(stored),
        os: OsLanguage::Supported(&["ko-KR"]),
        korean_integrity_failed: true,
    };

    let decision = decide(&inputs)?;

    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, Source::Baseline);
    assert_eq!(inputs.stored, Some("ko"));
    Ok(())
}

#[test]
fn gui_startup_is_blocked_when_the_english_baseline_is_not_registered() {
    let inputs = Inputs {
        registered: &["ko"],
        stored: None,
        os: OsLanguage::Supported(&["ko-KR"]),
        korean_integrity_failed: false,
    };

    assert_eq!(decide(&inputs), Err(DecisionError::RegistrationFailed));
}

#[test]
fn embedded_korean_integrity_failure_preserves_the_configured_language() -> Result<(), String> {
    use crate::i18n::{self, embedded};

    let mut resources = embedded::resources().map_err(|e| format!("{e:?}"))?;
    let korean = resources
        .iter_mut()
        .find(|resource| resource.language == "ko")
        .ok_or("missing Korean resource")?;
    korean.expected_hash[0] ^= 1;
    let declared = embedded::DECLARED_KEYS
        .iter()
        .map(|key| (*key).to_owned())
        .collect::<Vec<_>>();
    let bundles = i18n::Registry::register(&resources, &declared, &[], embedded::parse_bundle)
        .map_err(|e| format!("{e:?}"))?;
    let configured = bundles.configured_languages().collect::<Vec<_>>();
    let registered = bundles.languages().collect::<Vec<_>>();
    assert_eq!(registered, ["en"]);
    let warnings = bundles.integrity_warnings();
    assert_eq!(warnings, ["i18n.bundle_integrity_failed"]);

    let snapshot = SettingsSnapshot {
        schema_version: 1,
        revision: 7,
        values: [(DISPLAY_LANGUAGE_KEY.into(), json!("ko"))].into(),
    };
    let service = SettingsService::open(registry(&configured)?, MemoryStore(snapshot.clone()))
        .map_err(|e| format!("{e:?}"))?;
    let decision = decide(&Inputs {
        registered: &registered,
        stored: service
            .snapshot()
            .values
            .get(DISPLAY_LANGUAGE_KEY)
            .and_then(Value::as_str),
        os: OsLanguage::Supported(&["ko-KR"]),
        korean_integrity_failed: !warnings.is_empty(),
    })
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (decision.language.as_str(), decision.source),
        ("en", Source::Baseline)
    );
    assert_eq!(service.snapshot(), &snapshot);
    Ok(())
}
