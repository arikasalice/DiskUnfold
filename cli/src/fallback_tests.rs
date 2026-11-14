//! Translation-fallback records and language-independent machine output.

use std::{ffi::OsString, process::ExitCode};

use du_platform_os::{
    startup::SystemStartupCapabilities, support_environment::SystemSupportEnvironment,
};
use du_user_env::i18n::{Registry, embedded};

use crate::{
    OsPorts, Parsed, args, default_product_info, diagnostics, product_output, run_with_registry,
};

const FALLBACK_PREFIX: &str = "DiskUnfold: locale.translation_fallback: ";
const CAP: usize = diagnostics::MAX_TRANSLATION_FALLBACK_LINES;

/// Registers the built-in bundles with the given Korean keys left out.
///
/// The parser drops the rows after the verified bytes are parsed, so the
/// digest checks still pass and the Korean bundle simply lacks those keys.
fn registry_without_korean(omitted: &[&str]) -> Result<Registry, String> {
    let omitted: Vec<String> = omitted.iter().map(ToString::to_string).collect();
    let resources = embedded::resources().map_err(|error| format!("{error:?}"))?;
    let metadata = embedded::reason_metadata_resource().map_err(|error| format!("{error:?}"))?;
    let declared = embedded::declared_keys().map_err(|error| format!("{error:?}"))?;
    Registry::register_with_reason_metadata(
        &resources,
        &declared,
        &[],
        metadata.bytes,
        metadata.expected_hash,
        |bytes| {
            let mut bundle = embedded::parse_bundle(bytes)?;
            if bundle.language == "ko" {
                bundle.messages.retain(|(key, _)| !omitted.contains(key));
            }
            Ok(bundle)
        },
    )
    .map_err(|error| format!("{error:?}"))
}

struct Outcome {
    exit: ExitCode,
    stdout: Vec<u8>,
    stderr: String,
}

fn execute_with_stored(
    registry: &Registry,
    input: &[&str],
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
) -> Result<Outcome, String> {
    let args = args::parse(input.iter().map(OsString::from)).ok_or("arguments not parsed")?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_registry(
        Parsed { args, registry },
        default_product_info(),
        &OsPorts {
            environment: &SystemSupportEnvironment,
            access: &SystemStartupCapabilities,
        },
        read_stored,
        || None,
        &mut stdout,
        &mut stderr,
    );
    Ok(Outcome {
        exit,
        stdout,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

fn execute(registry: &Registry, input: &[&str]) -> Result<Outcome, String> {
    execute_with_stored(registry, input, || Ok(None))
}

fn fallback_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with(FALLBACK_PREFIX))
        .collect()
}

#[test]
fn missing_korean_text_is_reported_once_per_key_without_changing_the_exit_code()
-> Result<(), String> {
    let full = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let baseline = execute(&full, &["--lang", "ko", "support"])?;
    assert!(baseline.stderr.is_empty(), "{}", baseline.stderr);

    // `cli.product.unknown` is used for many values in one report; it must still be reported once.
    let registry = registry_without_korean(&["cli.product.unknown", "cli.support.title"])?;
    let outcome = execute(&registry, &["--lang", "ko", "support"])?;

    assert_eq!(outcome.exit, baseline.exit);
    assert_eq!(
        outcome.stdout.iter().filter(|byte| **byte == b'\n').count(),
        baseline
            .stdout
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
    );
    assert!(
        String::from_utf8_lossy(&outcome.stdout).contains("(번역 미확인)"),
        "the fallback text keeps the translation-unverified marker"
    );
    let lines = fallback_lines(&outcome.stderr);
    assert_eq!(lines.len(), 2, "{}", outcome.stderr);
    for key in ["cli.product.unknown", "cli.support.title"] {
        let suffix = format!("[key={key} language=ko]");
        assert_eq!(
            lines.iter().filter(|line| line.ends_with(&suffix)).count(),
            1,
            "{key}: {}",
            outcome.stderr
        );
    }
    for line in lines {
        // The cause comes from the Korean reason text; the line holds no other value.
        assert!(line.contains("선택한 언어에 번역이 없어"), "{line}");
    }
    assert_eq!(outcome.stderr.lines().count(), 2, "{}", outcome.stderr);
    Ok(())
}

#[test]
fn key_display_is_reported_with_the_key_and_language_only() -> Result<(), String> {
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let messages = product_output::Messages::new(&registry, "ko");
    let shown = messages.resolve("cli.test.undeclared");
    assert_eq!(shown.as_deref(), Some("cli.test.undeclared (번역 미확인)"));
    assert!(messages.resolve("cli.test.undeclared").is_some());

    let mut stderr = Vec::new();
    diagnostics::write_translation_fallbacks(&mut stderr, &registry);
    let stderr = String::from_utf8_lossy(&stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.ends_with("[key=cli.test.undeclared language=ko]\n"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn json_output_is_byte_identical_in_both_languages_even_with_missing_translations()
-> Result<(), String> {
    let full = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let json = ["support", "--format", "json"];
    let run = |registry: &Registry, language: &str| {
        let mut input = vec!["--lang", language];
        input.extend(json);
        execute(registry, &input)
    };
    let english = run(&full, "en")?;
    let korean = run(&full, "ko")?;
    assert_eq!(english.exit, ExitCode::SUCCESS);
    assert_eq!(korean.exit, ExitCode::SUCCESS);
    assert!(!english.stdout.is_empty());
    assert_eq!(english.stdout, korean.stdout);
    assert!(english.stderr.is_empty() && korean.stderr.is_empty());

    // A Korean bundle that lacks the product name warns on stderr; stdout does not change.
    let degraded = registry_without_korean(&["ui.product.name"])?;
    let degraded_korean = run(&degraded, "ko")?;
    assert_eq!(degraded_korean.exit, ExitCode::SUCCESS);
    assert_eq!(degraded_korean.stdout, english.stdout);
    assert_eq!(fallback_lines(&degraded_korean.stderr).len(), 1);

    // The same result in text form does differ by language.
    let text_en = execute(&full, &["--lang", "en", "support"])?;
    let text_ko = execute(&full, &["--lang", "ko", "support"])?;
    assert_ne!(text_en.stdout, text_ko.stdout);
    Ok(())
}

/// Records `count` Korean key displays and returns the warnings written for them.
fn warnings_for_undeclared_keys(count: usize) -> Result<Vec<String>, String> {
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let messages = product_output::Messages::new(&registry, "ko");
    for number in 0..count {
        assert!(
            messages
                .resolve(&format!("cli.test.undeclared_{number}"))
                .is_some()
        );
    }
    let mut stderr = Vec::new();
    diagnostics::write_translation_fallbacks(&mut stderr, &registry);
    Ok(String::from_utf8_lossy(&stderr)
        .lines()
        .map(ToOwned::to_owned)
        .collect())
}

#[test]
fn warnings_up_to_the_cap_are_all_written_without_a_summary() -> Result<(), String> {
    for count in [1, CAP - 1, CAP] {
        let lines = warnings_for_undeclared_keys(count)?;
        assert_eq!(lines.len(), count);
        assert!(
            lines
                .iter()
                .all(|line| line.contains("[key=cli.test.undeclared_"))
        );
        assert!(lines.iter().all(|line| !line.contains("omitted=")));
    }
    Ok(())
}

#[test]
fn warnings_beyond_the_cap_are_cut_and_summarized_in_one_line() -> Result<(), String> {
    for count in [CAP + 1, CAP + 37] {
        let lines = warnings_for_undeclared_keys(count)?;
        assert_eq!(lines.len(), CAP + 1, "{lines:?}");
        // The first records are listed in order; the cut ones are not named.
        for (number, line) in lines[..CAP].iter().enumerate() {
            assert!(
                line.ends_with(&format!("[key=cli.test.undeclared_{number} language=ko]")),
                "{line}"
            );
        }
        let summary = &lines[CAP];
        assert!(summary.starts_with(FALLBACK_PREFIX), "{summary}");
        assert!(
            summary.ends_with(&format!("[omitted={}]", count - CAP)),
            "{summary}"
        );
        assert!(
            !summary.contains("key=") && !summary.contains("language="),
            "{summary}"
        );
        assert!(
            summary.contains("번역이 없어 영어 문구나 문구 키로 표시된 문구가 더 있습니다"),
            "the summary text is localized: {summary}"
        );
    }
    Ok(())
}

#[test]
fn capped_warnings_keep_the_exit_code_and_output_and_carry_no_parameter_values()
-> Result<(), String> {
    let full = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let baseline = execute(&full, &["--lang", "ko", "support"])?;
    assert!(baseline.stderr.is_empty(), "{}", baseline.stderr);

    // Leave out every Korean support and product label: far more than the cap fall back.
    let english = embedded::resources()
        .map_err(|error| format!("{error:?}"))?
        .iter()
        .map(|resource| embedded::parse_bundle(resource.bytes))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{error:?}"))?
        .into_iter()
        .find(|bundle| bundle.language == "en")
        .ok_or("no English bundle")?;
    let omitted: Vec<&str> = english
        .messages
        .iter()
        .map(|(key, _)| key.as_str())
        .filter(|key| key.starts_with("cli.support.") || key.starts_with("cli.product."))
        .collect();
    let registry = registry_without_korean(&omitted)?;
    let outcome = execute(&registry, &["--lang", "ko", "support"])?;

    assert_eq!(outcome.exit, baseline.exit);
    assert_eq!(
        outcome.stdout.iter().filter(|byte| **byte == b'\n').count(),
        baseline
            .stdout
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
    );
    let lines: Vec<&str> = outcome.stderr.lines().collect();
    assert_eq!(lines.len(), CAP + 1, "{}", outcome.stderr);
    for line in &lines[..CAP] {
        // Only a key and a language follow the cause text: no value from the report.
        let tail = line
            .rsplit_once(" [key=")
            .map(|(_, tail)| tail)
            .unwrap_or_default();
        let (key, language) = tail.split_once(" language=").unwrap_or_default();
        assert!(key.starts_with("cli.") && language == "ko]", "{line}");
    }
    // The summary names a count and nothing else from the run.
    let summary = lines[CAP];
    let count = summary
        .rsplit_once(" [omitted=")
        .map(|(_, tail)| tail)
        .unwrap_or_default();
    assert!(
        count
            .strip_suffix(']')
            .is_some_and(|number| number.parse::<usize>().is_ok()),
        "{summary}"
    );
    Ok(())
}

/// Stored display language ko or en, read from an isolated data directory.
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[test]
fn stored_language_does_not_change_json_bytes_but_changes_text() -> Result<(), String> {
    use du_core_contracts::settings::{
        SettingsChanges, SettingsCommit, SettingsSource, SettingsStore,
    };
    use du_user_env::locale;

    use crate::startup_locale::stored_language_in_directory;

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let reference = execute(&registry, &["--lang", "en", "support", "--format", "json"])?;
    let mut scan_lines = Vec::new();
    for stored in ["ko", "en"] {
        let directory = std::env::temp_dir().join(format!(
            "diskunfold-cli-f13-machine-{}-{stored}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let _fixture = Fixture(directory.clone());
        let path = directory.join(du_platform_os::application_data::DATABASE_FILE);
        let mut database =
            du_app_db::AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?;
        database
            .commit(
                0,
                &SettingsChanges::from([(
                    locale::DISPLAY_LANGUAGE_KEY.into(),
                    Some(serde_json::Value::String(stored.to_owned())),
                )]),
                SettingsCommit {
                    source: SettingsSource::Screen,
                    timestamp_ms: 1,
                },
            )
            .map_err(|error| format!("{error:?}"))?;
        drop(database);
        assert_eq!(
            stored_language_in_directory(&directory),
            Ok(Some(stored.to_owned()))
        );

        let json = execute_with_stored(&registry, &["support", "--format", "json"], || {
            stored_language_in_directory(&directory)
        })?;
        assert_eq!(json.stdout, reference.stdout, "stored {stored}");
        assert!(json.stderr.is_empty(), "stored {stored}: {}", json.stderr);

        // A command that does read the stored language shows it in human text only.
        let scan = execute_with_stored(&registry, &["scan", "root"], || {
            stored_language_in_directory(&directory)
        })?;
        scan_lines.push(scan.stderr);
    }
    assert_ne!(scan_lines[0], scan_lines[1]);
    assert!(
        scan_lines[0].contains("지원하지 않는 인자"),
        "{}",
        scan_lines[0]
    );
    assert!(
        scan_lines[1].contains("unsupported arguments"),
        "{}",
        scan_lines[1]
    );
    Ok(())
}
