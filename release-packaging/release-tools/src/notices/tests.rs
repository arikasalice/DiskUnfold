use std::{error::Error, fs};

use serde_json::json;

use super::*;

fn package(name: &str, license: Option<&str>, text: Option<&str>) -> NoticePackage {
    NoticePackage {
        ecosystem: Ecosystem::Cargo,
        name: name.into(),
        version: "1.0.0".into(),
        license: license.map(str::to_owned),
        texts: text
            .map(|t| LicenseText {
                file: "LICENSE".into(),
                text: t.into(),
            })
            .into_iter()
            .collect(),
        authors: Vec::new(),
    }
}

fn policy() -> Result<DenyPolicy, String> {
    DenyPolicy::parse(
        "[licenses]\nallow = [\"MIT\", \"Apache-2.0\"]\n[bans]\ndeny = [{ name = \"ring\" }]\n",
    )
}

#[test]
fn rendering_is_deterministic_and_order_independent() {
    let a = package("a", Some("MIT"), Some("MIT text\r\nline  \r\n"));
    let b = package("b", Some("MIT"), Some("MIT text\nline\n\n"));
    let c = package("c", Some("Apache-2.0"), None);
    let first = render_notices(&[a.clone(), b.clone(), c.clone()]);
    let second = render_notices(&[c, b, a]);
    assert_eq!(first, second);
    assert!(!first.contains('\r'));
    // Identical texts after normalization appear once, with both users.
    assert_eq!(first.matches("License text ").count(), 1);
    assert!(first.contains("cargo a 1.0.0\n  cargo b 1.0.0"));
    assert!(first.contains("cargo c 1.0.0: Apache-2.0"));
    assert!(first.contains("Packages without a bundled license file"));
}

#[test]
fn policy_rejects_unknown_denied_and_banned_packages() -> Result<(), String> {
    let policy = policy()?;
    let packages = [
        package("ok", Some("MIT OR GPL-3.0-only"), None),
        package("gpl", Some("GPL-3.0-only"), None),
        package("both", Some("MIT AND GPL-3.0-only"), None),
        package("none", None, None),
        package("ring", Some("MIT"), None),
    ];
    let found: Vec<(&str, &str)> = check_policy(&packages, &policy)
        .iter()
        .map(|v| (v.reason_key, v.package.rsplit_once(':').map_or("", |p| p.1)))
        .map(|(k, p)| {
            (
                k,
                if p.starts_with("gpl") {
                    "gpl"
                } else if p.starts_with("both") {
                    "both"
                } else if p.starts_with("none") {
                    "none"
                } else {
                    "ring"
                },
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (LICENSE_DENIED, "both"),
            (LICENSE_DENIED, "gpl"),
            (LICENSE_UNKNOWN, "none"),
            (PACKAGE_BANNED, "ring"),
        ]
    );
    Ok(())
}

#[test]
fn repository_deny_toml_defines_the_policy() -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../deny.toml"))?;
    let policy = DenyPolicy::parse(&text)?;
    assert!(policy.allowed.contains("MIT"));
    assert!(policy.banned_crates.contains("ring"));
    assert!(DenyPolicy::parse("[licenses]\nallow = []\n").is_err());
    Ok(())
}

#[test]
fn cargo_closure_follows_normal_edges_of_registry_crates() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    for name in ["dep", "buildonly", "transitive"] {
        fs::create_dir(dir.path().join(name))?;
    }
    fs::write(dir.path().join("dep/LICENSE-MIT"), "dep license")?;
    fs::write(dir.path().join("dep/README.md"), "readme")?;
    let pkg = |name: &str, source: Option<&str>| {
        json!({"id": name, "name": name, "version": "1.0.0", "source": source, "license": "MIT",
               "license_file": null, "manifest_path": dir.path().join(name).join("Cargo.toml")})
    };
    let reg = Some("registry+https://github.com/rust-lang/crates.io-index");
    let edge =
        |pkg: &str, kind: Value| json!({"pkg": pkg, "dep_kinds": [{"kind": kind, "target": null}]});
    use serde_json::Value;
    let metadata = json!({
        "packages": [pkg("root", None), pkg("dep", reg), pkg("buildonly", reg), pkg("transitive", reg)],
        "resolve": {"nodes": [
            {"id": "root", "deps": [edge("dep", Value::Null), edge("buildonly", json!("build"))]},
            {"id": "dep", "deps": [edge("transitive", Value::Null)]},
            {"id": "buildonly", "deps": []},
            {"id": "transitive", "deps": []}
        ]}
    });
    let found = cargo_packages(&metadata, "root")?;
    let mut names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["dep", "transitive"]);
    let dep = found
        .iter()
        .find(|p| p.name == "dep")
        .ok_or("dep missing")?;
    assert_eq!(dep.texts.len(), 1);
    assert_eq!(dep.texts[0].file, "LICENSE-MIT");
    Ok(())
}

#[test]
fn npm_packages_skip_dev_and_read_installed_texts() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    fs::create_dir_all(dir.path().join("node_modules/@scope/lib"))?;
    fs::write(
        dir.path().join("node_modules/@scope/lib/LICENSE"),
        "scope license",
    )?;
    let lock = json!({"packages": {
        "": {"name": "gui"},
        "node_modules/@scope/lib": {"version": "2.0.0", "license": "MIT"},
        "node_modules/devtool": {"version": "1.0.0", "license": "MIT", "dev": true},
        "node_modules/bare": {"version": "3.0.0", "license": "ISC"}
    }});
    let found = npm_packages(&lock, dir.path())?;
    let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["@scope/lib", "bare"]);
    assert_eq!(found[0].texts[0].text, "scope license");
    assert!(found[1].texts.is_empty());
    assert_eq!(found[1].license.as_deref(), Some("ISC"));
    Ok(())
}
