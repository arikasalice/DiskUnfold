use super::*;
use crate::fixed_exclusion;
use du_core_contracts::share::{ExportField, Exportable, SecretCheckPort, SecretCheckUnavailable};
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Serialize)]
struct Candidate<'a> {
    note: &'a str,
    label: &'a str,
    content: &'a str,
    total: u64,
}
impl Exportable for Candidate<'_> {
    const FIELDS: &'static [ExportField] = &[
        ExportField {
            path: "/note",
            class: FieldClass::FreeText,
        },
        ExportField {
            path: "/label",
            class: FieldClass::ItemMetadata,
        },
        ExportField {
            path: "/content",
            class: FieldClass::FileContent,
        },
        ExportField {
            path: "/total",
            class: FieldClass::Aggregate,
        },
    ];
}
fn input(note: &str) -> Result<Intermediate, fixed_exclusion::Error> {
    fixed_exclusion::begin(&Candidate {
        note,
        label: "label",
        content: "contents",
        total: 8,
    })
}
struct Port {
    answer: SecretCheck,
    open_fails: bool,
    opens: Arc<AtomicUsize>,
    checks: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
struct Snapshot {
    answer: SecretCheck,
    checks: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl SecretCheckSnapshot for Snapshot {
    fn check(&self, _: &str) -> SecretCheck {
        self.checks.fetch_add(1, Ordering::SeqCst);
        self.answer
    }
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl SecretCheckPort for Port {
    fn open_snapshot(&self) -> Result<Box<dyn SecretCheckSnapshot>, SecretCheckUnavailable> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        if self.open_fails {
            return Err(SecretCheckUnavailable);
        }
        Ok(Box::new(Snapshot {
            answer: self.answer,
            checks: self.checks.clone(),
            drops: self.drops.clone(),
        }))
    }
}
fn port(answer: SecretCheck, open_fails: bool) -> Port {
    Port {
        answer,
        open_fails,
        opens: Arc::new(AtomicUsize::new(0)),
        checks: Arc::new(AtomicUsize::new(0)),
        drops: Arc::new(AtomicUsize::new(0)),
    }
}

#[test]
fn clean_text_is_retained_and_one_snapshot_lives_until_intermediate_is_dropped()
-> Result<(), fixed_exclusion::Error> {
    let mut port = port(SecretCheck::Clean, false);
    let snapshot = port.open_snapshot().ok();
    // A change after admission cannot change the open snapshot's decision.
    port.answer = SecretCheck::Contains;
    let checked = check(input("ordinary")?, snapshot);
    assert_eq!(checked.secret_check_excluded(), 0);
    assert_eq!(checked.limitation(), None);
    assert!(
        matches!(checked._fields.get("note"), Some(field) if field.class == FieldClass::FreeText)
    );
    assert!(
        matches!(checked._fields.get("label"), Some(field) if field.class == FieldClass::ItemMetadata)
    );
    assert!(!checked._fields.contains_key("content"));
    assert_eq!(port.opens.load(Ordering::SeqCst), 1);
    assert_eq!(port.checks.load(Ordering::SeqCst), 1);
    assert_eq!(port.drops.load(Ordering::SeqCst), 0);
    drop(checked);
    assert_eq!(port.drops.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn missing_contains_unavailable_and_open_failure_remove_only_free_text()
-> Result<(), fixed_exclusion::Error> {
    let ports = [
        port(SecretCheck::Contains, false),
        port(SecretCheck::Unavailable, false),
        port(SecretCheck::Clean, true),
    ];
    for port in &ports {
        let snapshot = port.open_snapshot().ok();
        let checked = check(input("text")?, snapshot);
        assert!(!checked._fields.contains_key("note"));
        assert!(checked._fields.contains_key("label"));
        assert_eq!(checked.secret_check_excluded(), 1);
        assert_eq!(checked.limitation(), Some(SECRET_CHECK_UNAVAILABLE));
    }
    let checked = check(input("text")?, None);
    assert!(!checked._fields.contains_key("note"));
    assert_eq!(checked.limitation(), Some(SECRET_CHECK_UNAVAILABLE));
    Ok(())
}

#[test]
fn fixed_exclusions_are_not_resubmitted_and_retries_are_independent()
-> Result<(), fixed_exclusion::Error> {
    let port = port(SecretCheck::Clean, false);
    let first_snapshot = port.open_snapshot().ok();
    let first = check(input("du_mcp_synthetic")?, first_snapshot);
    assert_eq!(first._fixed_excluded, 2);
    assert_eq!(port.checks.load(Ordering::SeqCst), 0);
    drop(first);
    let second_snapshot = port.open_snapshot().ok();
    let second = check(input("ordinary")?, second_snapshot);
    assert_eq!(port.opens.load(Ordering::SeqCst), 2);
    assert_eq!(port.checks.load(Ordering::SeqCst), 1);
    assert_eq!(second.secret_check_excluded(), 0);
    Ok(())
}

#[test]
fn non_string_free_text_is_dropped_without_calling_the_checker()
-> Result<(), fixed_exclusion::Error> {
    let port = port(SecretCheck::Clean, false);
    let snapshot = port.open_snapshot().ok();
    let input = fixed_exclusion::begin_json::<Candidate<'_>>(
        br#"{"note":42,"label":"label","content":"contents","total":8}"#,
    )?;
    let checked = check(input, snapshot);
    assert!(!checked._fields.contains_key("note"));
    assert_eq!(checked.limitation(), Some(SECRET_CHECK_UNAVAILABLE));
    assert_eq!(port.checks.load(Ordering::SeqCst), 0);
    Ok(())
}

struct SyntheticSnapshot;
impl SecretCheckSnapshot for SyntheticSnapshot {
    fn check(&self, text: &str) -> SecretCheck {
        if text.contains("synthetic-marker") {
            SecretCheck::Contains
        } else {
            SecretCheck::Clean
        }
    }
}

#[test]
fn rendered_json_uses_the_same_admission_snapshot() -> Result<(), fixed_exclusion::Error> {
    let mut source = port(SecretCheck::Clean, false);
    let snapshot = source.open_snapshot().ok();
    source.answer = SecretCheck::Contains;
    let checked = check(input("ordinary")?, snapshot);
    assert_eq!(
        checked.check_rendered_json(br#"{"text":"ordinary"}"#),
        Ok(())
    );
    assert_eq!(source.opens.load(Ordering::SeqCst), 1);
    assert!(source.checks.load(Ordering::SeqCst) >= 4);
    Ok(())
}

#[test]
fn invalid_unavailable_or_detected_output_never_passes() -> Result<(), fixed_exclusion::Error> {
    let checked = check(input("ordinary")?, Some(Box::new(SyntheticSnapshot)));
    for bytes in [
        b"\xff".as_slice(),
        b"{".as_slice(),
        br#"{"x":1,"x":2}"#.as_slice(),
        br#"{"x":"DU_MCP_example"}"#.as_slice(),
        br#"{"x":"du_\u006dcp_example"}"#.as_slice(),
        br#"{"x":"BEARER example"}"#.as_slice(),
        br#"{"x":"synthetic\u002dmarker"}"#.as_slice(),
        br#"{"synthetic\u002dmarker":0}"#.as_slice(),
        br#"{"nested":[{"ConnecTION_Key":"value"}]}"#.as_slice(),
        br#"{"connection\u005fkey":"value"}"#.as_slice(),
    ] {
        assert_eq!(checked.check_rendered_json(bytes), Err(RenderedSecretFound));
    }
    assert_eq!(
        RenderedSecretFound.reason_key(),
        "share.rendered_secret_found"
    );
    let missing = check(input("ordinary")?, None);
    assert_eq!(
        missing.check_rendered_json(br#"{"x":1}"#),
        Err(RenderedSecretFound)
    );
    for answer in [SecretCheck::Contains, SecretCheck::Unavailable] {
        let source = port(answer, false);
        let checked = check(input("ordinary")?, source.open_snapshot().ok());
        assert_eq!(
            checked.check_rendered_json(br#"{"x":1}"#),
            Err(RenderedSecretFound)
        );
    }
    Ok(())
}

#[test]
fn rendered_keys_cover_published_names_and_explicit_aliases() -> Result<(), fixed_exclusion::Error>
{
    let checked = check(input("ordinary")?, Some(Box::new(SyntheticSnapshot)));
    for name in [
        "authorization",
        "PASSWORD",
        "passphrase",
        "previewPassword",
        "verifier",
        "connectionKey",
        "apiKey",
        "token",
        "approvalToken",
        "secret",
        "connection_key",
        "credential",
        "preview_password",
        "approval_token",
        "api_key",
    ] {
        let bytes = format!("{{\"outer\":[{{\"{name}\":0}}]}}");
        assert_eq!(
            checked.check_rendered_json(bytes.as_bytes()),
            Err(RenderedSecretFound)
        );
    }
    assert_eq!(
        checked.check_rendered_json(br#"{"outer":{"api\u004bey":0}}"#),
        Err(RenderedSecretFound)
    );
    assert_eq!(
        checked.check_rendered_json(br#"{"password_hint":0}"#),
        Ok(())
    );
    Ok(())
}

#[test]
fn intermediate_json_compares_every_retained_path_and_value() -> Result<(), fixed_exclusion::Error>
{
    let source = port(SecretCheck::Clean, false);
    let checked = check(input("ordinary")?, source.open_snapshot().ok());
    assert_eq!(
        checked.check_intermediate_json(br#"{"total":8,"label":"label","note":"ordinary"}"#),
        Ok(())
    );
    assert_eq!(
        checked.check_intermediate_json(br#"{"label":"label","note":"\u006frdinary","total":8}"#),
        Ok(())
    );
    for bytes in [
        br#"{"total":8,"label":"label","note":"ordinary","extra":1}"#.as_slice(),
        br#"{"total":8,"label":"label"}"#.as_slice(),
        br#"{"total":9,"label":"label","note":"ordinary"}"#.as_slice(),
        br#"{"total":8,"label":"ordinary","note":"label"}"#.as_slice(),
        br#"{"total":8.0,"label":"label","note":"ordinary"}"#.as_slice(),
        br#"{"total":[8],"label":"label","note":"ordinary"}"#.as_slice(),
        br#"{"total":8,"label":"label","note":"ordinary","content":"contents"}"#.as_slice(),
    ] {
        assert_eq!(
            checked.check_intermediate_json(bytes),
            Err(IntermediateRenderError::Mismatch)
        );
    }
    for bytes in [br#"{"total":8,"total":8}"#.as_slice(), b"\xff".as_slice()] {
        assert_eq!(
            checked.check_intermediate_json(bytes),
            Err(IntermediateRenderError::SecretCheck)
        );
    }
    Ok(())
}

#[test]
fn empty_intermediate_and_removed_text_cannot_gain_leaves() -> Result<(), fixed_exclusion::Error> {
    struct Empty;
    impl Exportable for Empty {
        const FIELDS: &'static [ExportField] = &[];
    }
    let source = port(SecretCheck::Clean, false);
    let checked = check(
        fixed_exclusion::begin_json::<Empty>(b"{}")?,
        source.open_snapshot().ok(),
    );
    assert_eq!(checked.check_intermediate_json(b"{}"), Ok(()));
    assert_eq!(
        checked.check_intermediate_json(br#"{"x":1}"#),
        Err(IntermediateRenderError::Mismatch)
    );
    let unavailable = port(SecretCheck::Unavailable, false);
    let checked = check(input("ordinary")?, unavailable.open_snapshot().ok());
    // An unavailable snapshot blocks the final secret check even when free text was removed.
    assert_eq!(
        checked.check_intermediate_json(br#"{"label":"label","total":8}"#),
        Err(IntermediateRenderError::SecretCheck)
    );
    Ok(())
}

#[test]
fn rendered_check_accepts_the_snapshot_limit_and_rejects_larger_input()
-> Result<(), fixed_exclusion::Error> {
    let source = port(SecretCheck::Clean, false);
    let checked = check(input("ordinary")?, source.open_snapshot().ok());
    let mut json = vec![b' '; MAX_CHECK_BYTES];
    json[..2].copy_from_slice(b"{}");
    assert_eq!(checked.check_rendered_json(&json), Ok(()));
    let checks = source.checks.load(Ordering::SeqCst);
    json.push(b' ');
    assert_eq!(checked.check_rendered_json(&json), Err(RenderedSecretFound));
    assert_eq!(source.checks.load(Ordering::SeqCst), checks);
    Ok(())
}

#[test]
fn free_text_limit_uses_utf8_bytes_and_never_calls_snapshot_above_limit()
-> Result<(), fixed_exclusion::Error> {
    let source = port(SecretCheck::Clean, false);
    let exactly = "a".repeat(MAX_CHECK_BYTES);
    let checked = check(input(&exactly)?, source.open_snapshot().ok());
    assert!(checked._fields.contains_key("note"));
    assert_eq!(checked.limitation(), None);
    assert_eq!(source.checks.load(Ordering::SeqCst), 1);

    let too_large = "a".repeat(MAX_CHECK_BYTES + 1);
    let checked = check(input(&too_large)?, source.open_snapshot().ok());
    assert!(!checked._fields.contains_key("note"));
    assert!(checked._fields.contains_key("label"));
    assert_eq!(checked.secret_check_excluded(), 1);
    assert_eq!(checked.limitation(), Some(SECRET_CHECK_UNAVAILABLE));
    assert_eq!(source.checks.load(Ordering::SeqCst), 1);

    let multibyte = "가".repeat(MAX_CHECK_BYTES / 3 + 1);
    assert!(multibyte.chars().count() < MAX_CHECK_BYTES);
    let checked = check(input(&multibyte)?, source.open_snapshot().ok());
    assert!(!checked._fields.contains_key("note"));
    assert_eq!(checked.limitation(), Some(SECRET_CHECK_UNAVAILABLE));
    assert_eq!(source.checks.load(Ordering::SeqCst), 1);
    Ok(())
}
