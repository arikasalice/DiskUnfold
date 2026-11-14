use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use du_core_contracts::{
    Timestamp,
    locale_bundle::{LocaleMessageValue as Value, LocaleMissingMarker as Marker},
    locale_message::{LocaleMessagePort, LocaleMessageRequest, RenderedLocaleMessage},
    time_zone::{LocalTimeZone, TimeZonePort, TimeZoneUnavailable},
};

use super::{LocaleFormatSettings, RegistryMessagePort};
use crate::i18n::{BundleResource, Registry, embedded};

const PLURAL_KEY: &str = "ui.history.records_shown";
const PATH_KEY: &str = "ui.history.view_details_aria";

fn settings() -> LocaleFormatSettings {
    LocaleFormatSettings {
        number_format_id: "lang_default".into(),
        date_format_id: "lang_default".into(),
        time_zone_policy: "utc".into(),
        size_unit_system: "si".into(),
        size_unit: "auto".into(),
    }
}

/// Zone with daylight saving: UTC+1 from October to March, UTC+2 from April to
/// September. Fails every observation when `observable` is false.
struct SeasonalZone {
    observable: bool,
    observations: AtomicUsize,
}

impl SeasonalZone {
    fn new(observable: bool) -> Arc<Self> {
        Arc::new(Self {
            observable,
            observations: AtomicUsize::new(0),
        })
    }
}

impl TimeZonePort for SeasonalZone {
    fn observe(&self, instant: &Timestamp) -> Result<LocalTimeZone, TimeZoneUnavailable> {
        self.observations.fetch_add(1, Ordering::SeqCst);
        if !self.observable {
            return Err(TimeZoneUnavailable);
        }
        let summer = instant
            .as_str()
            .get(5..7)
            .is_some_and(|month| ("04"..="09").contains(&month));
        Ok(LocalTimeZone {
            identity: None,
            offset_seconds: if summer { 7200 } else { 3600 },
        })
    }
}

/// Registers the embedded bundles; `ko_without` removes one message row from Korean.
fn registry(ko_without: Option<&str>) -> Result<Arc<Registry>, String> {
    let altered: Vec<u8>;
    let mut resources = embedded::resources().map_err(|e| format!("{e:?}"))?;
    if let Some(key) = ko_without {
        let source = resources
            .iter()
            .find(|resource| resource.language == "ko")
            .ok_or("missing ko")?;
        let mut bundle: serde_json::Value =
            serde_json::from_slice(source.bytes).map_err(|e| e.to_string())?;
        bundle[5]
            .as_array_mut()
            .ok_or("messages are not an array")?
            .retain(|row| row[0].as_str() != Some(key));
        altered = serde_json::to_vec(&bundle).map_err(|e| e.to_string())?;
        for resource in &mut resources {
            if resource.language == "ko" {
                *resource = BundleResource {
                    bytes: &altered,
                    expected_hash: *blake3::hash(&altered).as_bytes(),
                    ..*resource
                };
            }
        }
    }
    let declared = embedded::declared_keys().map_err(|e| format!("{e:?}"))?;
    Registry::register(&resources, &declared, &[], embedded::parse_bundle)
        .map(Arc::new)
        .map_err(|e| format!("{e:?}"))
}

fn render(
    port: &dyn LocaleMessagePort,
    language: &str,
    key: &str,
    parameters: &[(&str, Value<'_>)],
) -> RenderedLocaleMessage {
    let parameters: BTreeMap<_, _> = parameters.iter().copied().collect();
    port.render(&LocaleMessageRequest {
        language,
        key,
        parameters: &parameters,
    })
}

fn port(ko_without: Option<&str>) -> Result<RegistryMessagePort, String> {
    Ok(RegistryMessagePort::new(
        registry(ko_without)?,
        settings(),
        SeasonalZone::new(true),
    ))
}

fn port_with_zone(policy: &str, zone: Arc<SeasonalZone>) -> Result<RegistryMessagePort, String> {
    let settings = LocaleFormatSettings {
        time_zone_policy: policy.into(),
        ..settings()
    };
    Ok(RegistryMessagePort::new(registry(None)?, settings, zone))
}

#[test]
fn complete_message_has_no_marker_and_selects_plural_form() -> Result<(), String> {
    let port = port(None)?;
    for (language, count, expected) in [
        ("en", 0, "0 records shown"),
        ("en", 1, "1 record shown"),
        ("en", 2, "2 records shown"),
        ("ko", 1, "기록 1개 표시"),
        ("ko", 2, "기록 2개 표시"),
    ] {
        let rendered = render(
            &port,
            language,
            PLURAL_KEY,
            &[("count", Value::Count(count))],
        );
        assert_eq!(rendered.text, expected);
        assert_eq!(rendered.marker, None);
        assert!(!rendered.unknown_parameter);
    }
    Ok(())
}

#[test]
fn key_missing_from_selected_language_falls_back_to_english() -> Result<(), String> {
    let port = port(Some(PLURAL_KEY))?;
    let rendered = render(&port, "ko", PLURAL_KEY, &[("count", Value::Count(1))]);
    assert_eq!(rendered.text, "1 record shown");
    assert_eq!(rendered.marker, Some(Marker::Fallback));
    assert!(!rendered.unknown_parameter);
    let intact = render(&port, "en", PLURAL_KEY, &[("count", Value::Count(1))]);
    assert_eq!(intact.marker, None);
    Ok(())
}

#[test]
fn key_unknown_to_every_language_is_displayed_as_the_key() -> Result<(), String> {
    let port = port(None)?;
    for language in ["ko", "en"] {
        let rendered = render(
            &port,
            language,
            "ui.fixture.not_registered",
            &[("count", Value::Count(3))],
        );
        assert_eq!(rendered.text, "ui.fixture.not_registered");
        assert_eq!(rendered.marker, Some(Marker::Key));
        assert!(!rendered.unknown_parameter);
    }
    Ok(())
}

#[test]
fn unregistered_language_uses_english_and_marks_fallback() -> Result<(), String> {
    let port = port(None)?;
    let rendered = render(&port, "fr", PLURAL_KEY, &[("count", Value::Count(2))]);
    assert_eq!(rendered.text, "2 records shown");
    assert_eq!(rendered.marker, Some(Marker::Fallback));
    Ok(())
}

#[test]
fn missing_or_mismatched_parameter_renders_unknown_value() -> Result<(), String> {
    let port = port(None)?;
    let name = ("name", Value::Name("report.txt"));
    let time = ("time", Value::Name("today"));

    let missing = render(&port, "ko", PATH_KEY, &[name, time]);
    assert_eq!(missing.text, "상세 보기: report.txt, today, 알 수 없는 값");
    assert!(missing.unknown_parameter);
    assert_eq!(missing.marker, None);

    let mismatched = render(
        &port,
        "en",
        PATH_KEY,
        &[name, time, ("target", Value::Name("/data/a"))],
    );
    assert_eq!(
        mismatched.text,
        "View details: report.txt, today, Unknown value"
    );
    assert!(mismatched.unknown_parameter);

    let wrong_count = render(&port, "en", PLURAL_KEY, &[("count", Value::Name("many"))]);
    assert_eq!(wrong_count.text, "Unknown value records shown");
    assert!(wrong_count.unknown_parameter);
    assert_eq!(
        render(&port, "en", PLURAL_KEY, &[]).text,
        "Unknown value records shown"
    );
    Ok(())
}

#[test]
fn path_and_name_values_are_inserted_without_change() -> Result<(), String> {
    let port = port(None)?;
    let path = r#"C:\Users\홍길동\{target}\a "b" {{x}}.txt"#;
    let name = "  report {1}  .txt ";
    let rendered = render(
        &port,
        "ko",
        PATH_KEY,
        &[
            ("name", Value::Name(name)),
            ("time", Value::Name("방금")),
            ("target", Value::Path(path)),
        ],
    );
    assert_eq!(rendered.text, format!("상세 보기: {name}, 방금, {path}"));
    assert!(!rendered.unknown_parameter);
    assert_eq!(rendered.marker, None);
    Ok(())
}

#[test]
fn counts_sizes_and_timestamps_follow_the_requested_language_profile() -> Result<(), String> {
    let port = port(None)?;
    let counts = [("count", Value::Count(1234)), ("total", Value::Count(5678))];
    let rendered = render(&port, "en", "ui.analysis.recent_unresolved_paths", &counts);
    assert!(rendered.text.starts_with("1,234 of 5,678 locations"));

    let size = render(
        &port,
        "en",
        "ui.visualization.lowerBound",
        &[("value", Value::Size(1500))],
    );
    assert_eq!(size.text, "At least 1.5 kB");

    let stamp = Value::Timestamp("2026-11-14T03:04:05.000Z");
    let english = render(&port, "en", "ui.analysis.volume_time", &[("time", stamp)]);
    assert_eq!(english.text, "11/14/2026 03:04:05 UTC");
    let korean = render(&port, "ko", "ui.analysis.volume_time", &[("time", stamp)]);
    assert_eq!(korean.text, "2026.11.14 03:04:05 UTC");

    let invalid = Value::Timestamp("2026-13-14T03:04:05.000Z");
    let rendered = render(&port, "en", "ui.analysis.volume_time", &[("time", invalid)]);
    assert_eq!(rendered.text, "Unknown value");
    assert!(rendered.unknown_parameter);
    Ok(())
}

#[test]
fn local_policy_observes_the_offset_for_each_timestamp() -> Result<(), String> {
    let zone = SeasonalZone::new(true);
    let port = port_with_zone("local", Arc::clone(&zone))?;
    let at = |text| {
        render(
            &port,
            "en",
            "ui.analysis.volume_time",
            &[("time", Value::Timestamp(text))],
        )
    };

    let winter = at("2026-01-15T12:00:00.000Z");
    assert_eq!(winter.text, "01/15/2026 13:00:00 UTC+01:00");
    let summer = at("2026-07-15T12:00:00.000Z");
    assert_eq!(summer.text, "07/15/2026 14:00:00 UTC+02:00");
    assert_eq!(zone.observations.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn failed_observation_displays_utc() -> Result<(), String> {
    let port = port_with_zone("local", SeasonalZone::new(false))?;
    let rendered = render(
        &port,
        "en",
        "ui.analysis.volume_time",
        &[("time", Value::Timestamp("2026-07-15T12:00:00.000Z"))],
    );
    assert_eq!(rendered.text, "07/15/2026 12:00:00 UTC");
    assert!(!rendered.unknown_parameter);
    Ok(())
}

#[test]
fn utc_policy_never_observes_the_zone() -> Result<(), String> {
    let zone = SeasonalZone::new(true);
    let port = port_with_zone("utc", Arc::clone(&zone))?;
    let rendered = render(
        &port,
        "en",
        "ui.analysis.volume_time",
        &[("time", Value::Timestamp("2026-07-15T12:00:00.000Z"))],
    );
    assert_eq!(rendered.text, "07/15/2026 12:00:00 UTC");
    assert_eq!(zone.observations.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn port_can_be_shared_across_threads() -> Result<(), String> {
    let port = Arc::new(port(None)?);
    let handle = {
        let port = Arc::clone(&port);
        std::thread::spawn(move || {
            render(&*port, "en", PLURAL_KEY, &[("count", Value::Count(1))]).text
        })
    };
    let text = handle.join().map_err(|_| "render thread panicked")?;
    assert_eq!(text, "1 record shown");
    Ok(())
}
