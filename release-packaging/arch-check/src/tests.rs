use super::*;
fn inspect(source: &str, krate: &str, module: &[&str]) -> Report {
    let mut report = Report::default();
    let mut edges = BTreeSet::new();
    let mut modules = BTreeSet::new();
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(error) => {
            report.add("fixture-parse", "fixture.rs", error.to_string());
            return report;
        }
    };
    let mut scan = Scan {
        report: &mut report,
        crate_name: krate,
        file: Path::new("fixture.rs"),
        module: module.iter().map(|s| s.to_string()).collect(),
        edges: &mut edges,
        modules: &mut modules,
    };
    for item in &file.items {
        scan.visit_item(item);
    }
    report
}
#[test]
fn catches_adversarial_rust_syntax() {
    let report = inspect(
        "use crate::api::{Thing as Alias, Other}; unsafe fn bad() {} pub fn export() {} mod helpers {}",
        "core-contracts",
        &["domain"],
    );
    for rule in ["layer", "unsafe", "core-public-function", "module-name"] {
        assert!(
            report.diagnostics.iter().any(|d| d.rule == rule),
            "missing {rule}"
        );
    }
}
#[test]
fn rejects_reverse_dependencies_in_trait_and_struct_paths() {
    for source in [
        "impl crate::api::Service for Local {}",
        "fn register<T: crate::api::Service>() {}",
        "fn make() { let _ = crate::api::Request { value: 0 }; }",
        "fn read(value: crate::ports::Request) { let crate::api::Request { value } = value; }",
    ] {
        let report = inspect(source, "engine-api", &["domain"]);
        assert!(
            report.diagnostics.iter().any(|d| d.rule == "layer"),
            "missed reverse dependency in {source}"
        );
    }
}
#[test]
fn platform_unsafe_and_allowed_layers_pass() {
    let report = inspect(
        "use crate::ports::Thing; impl crate::ports::Service for Local {} unsafe fn permitted() {}",
        "platform-os",
        &["domain"],
    );
    assert!(report.diagnostics.is_empty());
}
#[test]
fn detects_constructor_and_complexity() {
    let mut source = String::from("fn new(a:u8,b:u8,c:u8,d:u8,e:u8) { ");
    for _ in 0..21 {
        source.push_str("if true {} ");
    }
    source.push('}');
    let report = inspect(&source, "engine-api", &[]);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.rule == "constructor-dependencies")
    );
    assert!(report.diagnostics.iter().any(|d| d.rule == "complexity"));
}

#[test]
fn dependency_kinds_and_aliases_cannot_bypass_boundaries() {
    let meta = Metadata {
        workspace_members: vec!["a".into(), "b".into()],
        packages: vec![
            Package {
                id: "a".into(),
                name: "du-cli".into(),
                manifest_path: "cli/Cargo.toml".into(),
                dependencies: vec![
                    Dependency {
                        name: "du-change-executor".into(),
                        kind: Some("dev".into()),
                        rename: Some("hidden".into()),
                    },
                    Dependency {
                        name: "du-change-executor".into(),
                        kind: Some("build".into()),
                        rename: None,
                    },
                ],
            },
            Package {
                id: "b".into(),
                name: "du-change-executor".into(),
                manifest_path: "change-executor/Cargo.toml".into(),
                dependencies: vec![],
            },
        ],
    };
    let policy = Policy {
        crates: BTreeMap::from([("cli".into(), vec![]), ("change-executor".into(), vec![])]),
    };
    let mut report = Report::default();
    dependencies(&meta, &policy, &mut report);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.rule == "executor-boundary")
            .count(),
        2
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.rule == "crate-dependency")
            .count(),
        2
    );
    assert_eq!(report.metrics.crate_direct_dependencies["cli"], 1);
}

#[test]
fn remapped_paths_and_macro_unsafe_are_rejected() {
    let source = r#"#[path = "../../hidden.rs"] mod domain;
        #[cfg_attr(feature = "x", path = "other.rs")] mod ports;
        macro_rules! hidden { () => {{ unsafe { operation() } }} }
    "#;
    let report = inspect(source, "engine-api", &[]);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.rule == "module-path")
            .count(),
        2
    );
    assert!(report.diagnostics.iter().any(|d| d.rule == "unsafe"));
    let allowed = inspect(
        "macro_rules! hidden { () => { unsafe { operation() } } }",
        "platform-os",
        &[],
    );
    assert!(allowed.diagnostics.is_empty());
}

#[test]
fn optional_metadata_dependency_is_still_checked() -> Result<(), serde_json::Error> {
    let json =
        r#"{"name":"du-change-executor","kind":"build","rename":"camouflaged","optional":true}"#;
    let dependency: Dependency = serde_json::from_str(json)?;
    let meta = Metadata {
        workspace_members: vec!["a".into(), "b".into()],
        packages: vec![
            Package {
                id: "a".into(),
                name: "du-cli".into(),
                manifest_path: "cli/Cargo.toml".into(),
                dependencies: vec![dependency],
            },
            Package {
                id: "b".into(),
                name: "du-change-executor".into(),
                manifest_path: "change-executor/Cargo.toml".into(),
                dependencies: vec![],
            },
        ],
    };
    let policy = Policy {
        crates: BTreeMap::from([("cli".into(), vec![]), ("change-executor".into(), vec![])]),
    };
    let mut report = Report::default();
    dependencies(&meta, &policy, &mut report);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.rule == "executor-boundary")
    );
    Ok(())
}
