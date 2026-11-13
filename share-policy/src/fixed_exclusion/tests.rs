use super::*;
use du_core_contracts::share::ExportField;

#[derive(Serialize)]
struct Candidate<'a> {
    total: u64,
    secret: &'a str,
    content: &'a str,
    note: &'a str,
    #[serde(rename = "Authorization")]
    authorization: &'a str,
}
impl Exportable for Candidate<'_> {
    const FIELDS: &'static [ExportField] = &[
        ExportField {
            path: "/total",
            class: FieldClass::Aggregate,
        },
        ExportField {
            path: "/secret",
            class: FieldClass::Secret,
        },
        ExportField {
            path: "/content",
            class: FieldClass::FileContent,
        },
        ExportField {
            path: "/note",
            class: FieldClass::FreeText,
        },
        ExportField {
            path: "/Authorization",
            class: FieldClass::ItemMetadata,
        },
    ];
}
fn candidate<'a>(note: &'a str) -> Candidate<'a> {
    Candidate {
        total: 4,
        secret: "secret",
        content: "file contents",
        note,
        authorization: "sensitive",
    }
}

#[test]
fn actual_serialization_is_classified_before_fixed_exclusion() -> Result<(), Error> {
    let result = begin(&candidate("ordinary free text"))?;
    assert_eq!(result.excluded, 3);
    assert_eq!(result.fields.len(), 2);
    assert_eq!(
        result.fields.get("note").map(|entry| &entry.value),
        Some(&Value::from("ordinary free text"))
    );
    assert!(
        matches!(result.fields.get("note"), Some(entry) if entry.class == FieldClass::FreeText)
    );
    // Retention here is not a secret check or an export permission.
    Ok(())
}

#[test]
fn fixed_secret_patterns_are_excluded_independently_of_declared_class() -> Result<(), Error> {
    for text in ["before du_mcp_test", "before BEARER token"] {
        let result = begin(&candidate(text))?;
        assert_eq!(result.excluded, 4);
        assert!(!result.fields.contains_key("note"));
    }
    Ok(())
}

#[test]
fn a_secret_name_declared_as_aggregate_is_still_removed() -> Result<(), Error> {
    struct Misclassified;
    impl Exportable for Misclassified {
        const FIELDS: &'static [ExportField] = &[
            ExportField {
                path: "/Authorization",
                class: FieldClass::Aggregate,
            },
            ExportField {
                path: "/total",
                class: FieldClass::Aggregate,
            },
        ];
    }

    for input in [
        br#"{"Authorization":"synthetic-sensitive","total":4}"#.as_slice(),
        br#"{"\u0041uthorization":"synthetic-sensitive","total":4}"#.as_slice(),
    ] {
        let result = begin_json::<Misclassified>(input)?;
        assert_eq!(result.excluded, 1);
        assert!(!result.fields.contains_key("Authorization"));
        assert_eq!(
            result.fields.get("total").map(|field| &field.value),
            Some(&Value::from(4))
        );
    }
    let unexpected = br#"{"Authorization":"synthetic-sensitive","total":4,"unclassified":"synthetic-sensitive"}"#;
    let error = begin_json::<Misclassified>(unexpected).err();
    assert_eq!(error, Some(Error::DeclarationMismatch));
    assert!(!format!("{error:?}").contains("synthetic-sensitive"));
    Ok(())
}

#[test]
fn published_secret_names_and_compatible_aliases_are_excluded_after_decoding() -> Result<(), Error>
{
    struct Names;
    impl Exportable for Names {
        const FIELDS: &'static [ExportField] = &[
            ExportField {
                path: "/Authorization",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/PASSWORD",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/Passphrase",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/previewPassword",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/verifier",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/connectionKey",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/apiKey",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/token",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/approvalToken",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/secret",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/connection_key",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/credential",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/preview_password",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/approval_token",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/api_key",
                class: FieldClass::ItemMetadata,
            },
            ExportField {
                path: "/password_hint",
                class: FieldClass::ItemMetadata,
            },
        ];
    }
    let input = br#"{"Authorization":0,"PASSWORD":0,"Passphrase":0,"preview\u0050assword":0,"verifier":0,"connectionKey":0,"apiKey":0,"token":0,"approvalToken":0,"secret":0,"connection_key":0,"credential":0,"preview_password":0,"approval_token":0,"api_key":0,"password_hint":0}"#;
    let result = begin_json::<Names>(input)?;
    assert_eq!(result.excluded, 15);
    assert_eq!(result.fields.len(), 1);
    assert!(result.fields.contains_key("password_hint"));
    Ok(())
}

#[test]
fn invalid_or_ambiguous_json_never_returns_intermediate() {
    for input in [
        b"{".as_slice(),
        br#"{"x":1,"x":2}"#.as_slice(),
        br#"{"x":1,"\u0078":2}"#.as_slice(),
    ] {
        assert!(matches!(
            begin_json::<Candidate<'_>>(input),
            Err(Error::InvalidJson)
        ));
    }
    for input in [
        br#"{"total":4}"#.as_slice(),
        br#"{"total":4,"secret":"x","content":"x","note":"x","Authorization":"x","extra":1}"#
            .as_slice(),
    ] {
        assert!(matches!(
            begin_json::<Candidate<'_>>(input),
            Err(Error::DeclarationMismatch)
        ));
    }
    assert!(matches!(
        begin_json::<Candidate<'_>>(br#"{"total":[4]}"#),
        Err(Error::UnsupportedShape)
    ));
}

struct Duplicate;
impl Exportable for Duplicate {
    const FIELDS: &'static [ExportField] = &[
        ExportField {
            path: "/x",
            class: FieldClass::Aggregate,
        },
        ExportField {
            path: "/x",
            class: FieldClass::Secret,
        },
    ];
}
struct Empty;
impl Exportable for Empty {
    const FIELDS: &'static [ExportField] = &[];
}
struct DuplicateSerialized;
impl Serialize for DuplicateSerialized {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("x", &1)?;
        map.serialize_entry("x", &2)?;
        map.end()
    }
}
impl Exportable for DuplicateSerialized {
    const FIELDS: &'static [ExportField] = &[ExportField {
        path: "/x",
        class: FieldClass::Aggregate,
    }];
}

#[test]
fn duplicate_declarations_are_rejected_and_empty_object_is_supported() {
    assert!(matches!(
        begin_json::<Duplicate>(br#"{"x":1}"#),
        Err(Error::DeclarationMismatch)
    ));
    assert!(
        matches!(begin_json::<Empty>(b"{}"), Ok(value) if value.fields.is_empty() && value.excluded == 0)
    );
    assert!(matches!(
        begin(&DuplicateSerialized),
        Err(Error::InvalidJson)
    ));
}
