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
