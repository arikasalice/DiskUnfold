use super::*;
use crate::notices::{Ecosystem, LicenseText};

const MIT_LICENSE: &str = "MIT License\n\nCopyright (c) 2026 someone\n";

fn package(
    ecosystem: Ecosystem,
    name: &str,
    license: &str,
    text: Option<&str>,
    authors: &[&str],
) -> NoticePackage {
    NoticePackage {
        ecosystem,
        name: name.into(),
        version: "1.0.0".into(),
        license: Some(license.into()),
        texts: text
            .map(|t| LicenseText {
                file: "LICENSE".into(),
                text: t.into(),
            })
            .into_iter()
            .collect(),
        authors: authors.iter().map(|a| (*a).to_owned()).collect(),
    }
}

#[test]
fn output_is_deterministic_and_groups_by_license() -> Result<(), String> {
    let a = package(
        Ecosystem::Cargo,
        "a",
        "MIT/Apache-2.0",
        Some("Copyright (c) 2015  Alice\n"),
        &[],
    );
    let b = package(Ecosystem::Npm, "b", "MIT OR Apache-2.0", None, &["Bob"]);
    let c = package(
        Ecosystem::Cargo,
        "c",
        "(MIT OR Apache-2.0) AND Unicode-3.0",
        None,
        &[],
    );
    let d = package(
        Ecosystem::Cargo,
        "d",
        "Apache-2.0 WITH LLVM-exception OR MIT",
        None,
        &[],
    );
    let first = render(&[a.clone(), b.clone(), c.clone(), d.clone()], MIT_LICENSE)?;
    let second = render(&[d, c, b, a], MIT_LICENSE)?;
    assert_eq!(first, second);
    assert!(!first.contains('\r'));
    assert!(first.starts_with(
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
    ));
    assert!(first.contains("\nFiles: *\nCopyright: 2026 someone\nLicense: MIT\n"));
    // MIT/Apache-2.0 and MIT OR Apache-2.0 are one group with both packages.
    assert!(first.contains(
        "Files: third-party/cargo/a-1.0.0/*\n third-party/npm/b-1.0.0/*\nCopyright: Authors of npm b 1.0.0: Bob\n Copyright (c) 2015 Alice\nLicense: MIT or Apache-2.0\n"
    ));
    assert!(first.contains("\nLicense: MIT or Apache-2.0, and Unicode-3.0\n"));
    assert!(first.contains("\nLicense: Apache-2.0 with LLVM-exception or MIT\n"));
    assert!(first.contains("No copyright statement or author recorded for cargo c 1.0.0"));
    for id in ["MIT", "Apache-2.0", "Unicode-3.0", "LLVM-exception"] {
        assert_eq!(
            first.matches(&format!("\nLicense: {id}\n ")).count(),
            1,
            "{id}"
        );
    }
    // Debian's own copy is referenced for Apache-2.0; other texts stay in full.
    assert!(first.contains(
        "\nLicense: Apache-2.0\n On Debian systems, the complete text of the Apache License 2.0 can be\n found in /usr/share/common-licenses/Apache-2.0.\n"
    ));
    assert!(!first.contains("Version 2.0, January 2004"));
    assert!(first.contains("Permission is hereby granted, free of charge"));
    Ok(())
}

#[test]
fn unusable_inputs_are_rejected() {
    let unknown = package(Ecosystem::Cargo, "x", "Not-A-License", None, &[]);
    assert!(render(&[unknown], MIT_LICENSE).is_err());
    let none = NoticePackage {
        license: None,
        ..package(Ecosystem::Npm, "x", "MIT", None, &[])
    };
    assert!(render(&[none], MIT_LICENSE).is_err());
    assert!(render(&[], "Apache License").is_err());
    let nested = package(Ecosystem::Cargo, "n", "MIT AND (Zlib OR ISC)", None, &[]);
    assert!(render(&[nested], MIT_LICENSE).is_err());
}

#[test]
fn only_real_copyright_statements_are_kept() {
    assert_eq!(
        copyright_line("  Copyright   2019 The Foo   Authors", "").as_deref(),
        Some("Copyright 2019 The Foo Authors")
    );
    assert!(copyright_line("Copyright (c) 2019 Foo", "").is_some());
    assert!(copyright_line("Copyright © 1991-2025 Unicode, Inc.", "").is_some());
    assert!(copyright_line("Copyright (c) <year> <copyright holders>", "").is_none());
    assert!(copyright_line("Copyright [yyyy] [name of copyright owner]", "").is_none());
    assert!(copyright_line("Copyright License. Subject to the terms", "").is_none());
    assert_eq!(
        copyright_line("Copyright (c) 2016--2017", "  Jane Doe").as_deref(),
        Some("Copyright (c) 2016--2017 Jane Doe")
    );
}
