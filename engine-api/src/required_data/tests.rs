use super::*;

const FORMATS: [&str; 6] = [
    "treemap",
    "bar",
    "trend",
    "waterfall",
    "histogram",
    "sankey",
];

/// One logical source fixture; this is not a production evidence provider.
fn source_key(suffix: u8) -> SourceKey {
    let mut lineage = [0x00; 16];
    lineage[0] = 0x01;
    lineage[1] = 0x90;
    lineage[6] = 0x70;
    lineage[8] = 0x80;
    lineage[15] = suffix;
    let mut revision = lineage;
    revision[14] = suffix;
    SourceKey::fixture(Id::from_v7_bytes(lineage), Id::from_v7_bytes(revision))
}

/// One logical pinned-source fixture; it is not a production evidence provider.
fn complete_fixture() -> FactValues {
    FactValues {
        folder_hierarchy: Evidence::Known(()),
        item_sizes: Evidence::Known(()),
        numeric_values: Evidence::Known(()),
        trend_observations: Evidence::Known(2),
        comparison: Evidence::Known(()),
        histogram_bins: Evidence::Known(()),
        attribution: Evidence::Known(Attribution {
            has_negative_flow: false,
        }),
    }
}

fn pinned(values: FactValues) -> (SourceKey, PinnedFacts) {
    let source = source_key(1);
    let facts = Facts {
        source: source.clone(),
        values,
    };
    let pinned = PinnedFacts::bind_fixture(source.clone(), facts);
    (source, pinned)
}

fn registry_formats() -> Vec<String> {
    FORMATS.iter().map(|format| (*format).to_owned()).collect()
}

#[test]
fn every_published_format_is_available_when_its_pinned_facts_are_present() {
    let (source, pinned) = pinned(complete_fixture());

    for format in FORMATS {
        assert_eq!(
            evaluate(format, &source, &pinned),
            Ok(Evaluation::Available)
        );
    }
    let formats = registry_formats();
    let availability = format_availability(&formats, &source, &pinned).unwrap();
    assert_eq!(availability.available, FORMATS.to_vec());
    assert!(availability.unresolved.is_empty());
}

#[test]
fn each_published_format_returns_its_first_missing_requirement_reason() {
    let cases = [
        (
            "treemap",
            Reason::NoItemSizes,
            FactValues {
                folder_hierarchy: Evidence::Absent,
                item_sizes: Evidence::Unknown,
                ..complete_fixture()
            },
        ),
        (
            "bar",
            Reason::NoItemSizes,
            FactValues {
                numeric_values: Evidence::Absent,
                ..complete_fixture()
            },
        ),
        (
            "trend",
            Reason::SingleObservation,
            FactValues {
                trend_observations: Evidence::Known(1),
                ..complete_fixture()
            },
        ),
        (
            "waterfall",
            Reason::NoBaseline,
            FactValues {
                comparison: Evidence::Absent,
                ..complete_fixture()
            },
        ),
        (
            "histogram",
            Reason::NoItemSizes,
            FactValues {
                item_sizes: Evidence::Absent,
                histogram_bins: Evidence::Unknown,
                ..complete_fixture()
            },
        ),
        (
            "sankey",
            Reason::NoAttribution,
            FactValues {
                attribution: Evidence::Absent,
                ..complete_fixture()
            },
        ),
    ];

    for (format, reason, facts) in cases {
        let (source, pinned) = pinned(facts);
        assert_eq!(
            evaluate(format, &source, &pinned),
            Ok(Evaluation::Unavailable(reason)),
            "{format}"
        );
        assert_eq!(reason.key(), expected_reason_key(reason));
    }
}

#[test]
fn known_negative_sankey_flow_is_a_distinct_first_failure() {
    let facts = FactValues {
        attribution: Evidence::Known(Attribution {
            has_negative_flow: true,
        }),
        ..complete_fixture()
    };
    let (source, pinned) = pinned(facts);

    assert_eq!(
        evaluate("sankey", &source, &pinned),
        Ok(Evaluation::Unavailable(Reason::NegativeFlow))
    );
}

#[test]
fn unknown_evidence_is_not_reported_as_an_unavailable_reason() {
    let facts = FactValues {
        folder_hierarchy: Evidence::Unknown,
        item_sizes: Evidence::Absent,
        ..complete_fixture()
    };
    let (source, pinned) = pinned(facts);

    assert_eq!(
        evaluate("treemap", &source, &pinned),
        Err(Error::EvidenceUnknown)
    );
    let formats = registry_formats();
    let availability = format_availability(&formats, &source, &pinned).unwrap();
    assert!(availability.unresolved.contains(&"treemap"));
}

#[test]
fn facts_from_a_different_result_revision_are_rejected() {
    let (_source, pinned) = pinned(complete_fixture());
    let other_source = source_key(2);

    assert_eq!(
        evaluate("bar", &other_source, &pinned),
        Err(Error::SourceMismatch)
    );
}

#[test]
fn evidence_labeled_from_another_revision_is_rejected_during_binding() {
    let source = source_key(1);
    let facts = Facts {
        source: source_key(2),
        values: complete_fixture(),
    };
    let pinned = PinnedFacts::bind_fixture(source.clone(), facts);

    assert_eq!(
        evaluate("bar", &source, &pinned),
        Err(Error::SourceMismatch)
    );
}

#[test]
fn unregistered_format_is_distinct_from_unresolved_evidence() {
    let (source, pinned) = pinned(complete_fixture());

    assert_eq!(
        evaluate("scatter", &source, &pinned),
        Err(Error::UnknownFormat)
    );
}

#[test]
fn unavailable_format_list_is_recomputed_from_the_same_pinned_facts() {
    let facts = FactValues {
        attribution: Evidence::Absent,
        ..complete_fixture()
    };
    let (source, pinned) = pinned(facts);

    let formats = registry_formats();
    let availability = format_availability(&formats, &source, &pinned).unwrap();
    assert_eq!(
        availability.available,
        vec!["treemap", "bar", "trend", "waterfall", "histogram"]
    );
    assert!(availability.unresolved.is_empty());
}

fn expected_reason_key(reason: Reason) -> &'static str {
    match reason {
        Reason::NoBaseline => "viz.unavailable.no_baseline",
        Reason::SingleObservation => "viz.unavailable.single_observation",
        Reason::NoAttribution => "viz.unavailable.no_attribution",
        Reason::NoItemSizes => "viz.unavailable.no_item_sizes",
        Reason::NegativeFlow => "viz.unavailable.negative_flow",
    }
}
