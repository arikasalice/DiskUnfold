use super::*;
use crate::{IngestSink, Observation};
use du_core_contracts::{Id, result::ResultCompleteness};

const ATTRIBUTES: [ObservedAttribute; 4] = [
    ObservedAttribute::ModifiedAt,
    ObservedAttribute::Allocated,
    ObservedAttribute::LinkCount,
    ObservedAttribute::SpaceIdentity,
];

fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}

#[test]
fn every_reason_number_maps_back_to_its_constant() {
    for attribute in ATTRIBUTES {
        for reason in REASONS {
            let code = code(attribute, reason);
            assert_eq!(code.map(attribute_of), Some(attribute as u8));
            assert_eq!(code.and_then(reason_of), Some(reason));
        }
        assert_eq!(code(attribute, "os.other"), None);
    }
    assert_eq!(reason_of(0), None);
}

#[test]
fn a_bound_table_keeps_the_last_record_of_each_attribute() {
    let modified = ObservedAttribute::ModifiedAt;
    let allocated = ObservedAttribute::Allocated;
    let row = |attribute, reason| code(attribute, reason).unwrap_or_default();
    // Nodes 0..3 are items 2, 0 and 1.
    let table = AttributeReasons::bind(
        &[2, 0, 1],
        &[
            (0, row(modified, attribute_reason::QUERY_FAILED)),
            (1, row(allocated, attribute_reason::PLACEHOLDER)),
            (0, row(modified, attribute_reason::UNSUPPORTED)),
            (0, row(allocated, attribute_reason::OTHER_VOLUME)),
            (9, row(modified, attribute_reason::QUERY_FAILED)),
        ],
    );
    assert_eq!(&*table.items, &[0, 2, 2]);
    assert_eq!(
        table.reason(2, modified),
        Some(attribute_reason::UNSUPPORTED)
    );
    assert_eq!(
        table.reason(2, allocated),
        Some(attribute_reason::OTHER_VOLUME)
    );
    assert_eq!(
        table.reason(0, allocated),
        Some(attribute_reason::PLACEHOLDER)
    );
    assert_eq!(table.reason(0, modified), None);
    assert_eq!(table.reason(1, allocated), None);
    assert_eq!(table.heap_bytes(), 15);
}

/// FR2.HD1 memory bound: two unknown attributes on each of a million items cost five
/// bytes a row in the publication, nothing for an item whose attributes are known.
#[test]
fn a_million_items_with_two_unknown_attributes_hold_ten_bytes_each() {
    const ITEMS: u32 = 1_000_000;
    let item_of_node: Vec<u32> = (0..ITEMS * 2).collect();
    let mut recorded = Vec::with_capacity(ITEMS as usize * 2);
    for node in 0..ITEMS {
        for (attribute, reason) in [
            (ObservedAttribute::Allocated, attribute_reason::UNSUPPORTED),
            (ObservedAttribute::LinkCount, attribute_reason::QUERY_FAILED),
        ] {
            recorded.push((node * 2, code(attribute, reason).unwrap_or_default()));
        }
    }
    let table = AttributeReasons::bind(&item_of_node, &recorded);
    assert_eq!(table.heap_bytes(), ITEMS as usize * 10);
    assert_eq!(
        table.reason(ITEMS * 2 - 2, ObservedAttribute::LinkCount),
        Some(attribute_reason::QUERY_FAILED)
    );
    assert_eq!(table.reason(1, ObservedAttribute::Allocated), None);
}

#[test]
fn a_version_reports_the_reasons_recorded_for_its_items() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(e)?;
    build.add_directory_at(0, Vec::new()).map_err(e)?;
    let file = vec![b"f".to_vec()];
    build
        .add_file_at(
            0,
            Observation {
                path: file.clone(),
                size: None,
            },
        )
        .map_err(e)?;
    let allocated = ObservedAttribute::Allocated;
    build
        .set_attribute_unknown(0, &file, allocated, attribute_reason::PLACEHOLDER)
        .map_err(e)?;
    // An unregistered reason is not kept and an unknown path is refused.
    build
        .set_attribute_unknown(0, &file, ObservedAttribute::LinkCount, "os.other")
        .map_err(e)?;
    let missing = [b"missing".to_vec()];
    assert_eq!(
        build.set_attribute_unknown(0, &missing, allocated, attribute_reason::PLACEHOLDER),
        Err(BuildError::InvalidTree)
    );
    let version = build.seal(id(2), ResultCompleteness::Complete).map_err(e)?;
    assert_eq!(
        version.attribute_unknown_reason(1, allocated),
        Some(attribute_reason::PLACEHOLDER)
    );
    assert_eq!(
        version.attribute_unknown_reason(1, ObservedAttribute::LinkCount),
        None
    );
    assert_eq!(version.attribute_unknown_reason(0, allocated), None);
    assert_eq!(version.attribute_reason_bytes(), 5);
    Ok(())
}
