//! F02 #300: the legend classes of a whole result, every scope root counted together.
use du_analysis_index::visualization::type_legend::{ClassTotal, result_totals, scope_totals};
use du_analysis_index::{FileIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn error(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

fn root(name: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![1],
        file_id: name.to_vec(),
    }
}

fn total(count: u64, bytes: u64) -> ClassTotal {
    ClassTotal {
        count,
        bytes,
        lower_bound: false,
    }
}

#[test]
fn a_file_linked_from_two_roots_adds_its_allocation_once_and_classes_come_in_legend_order()
-> Result<(), String> {
    let sink = IngestSink::default();
    let mut builder = sink
        .open_forest(Id::from_v7_bytes([1; 16]), vec![root(b"/a"), root(b"/b")])
        .map_err(error)?;
    builder.add_directory_at(0, vec![]).map_err(error)?;
    builder.add_directory_at(1, vec![]).map_err(error)?;
    let allocated = SizeValue {
        basis: SizeBasis::Allocated,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: 4096_u64.into(),
        },
    };
    // a.pdf under /a and b.pdf under /b are one file; v.mp4 is another.
    for (root, name, file) in [(0, "v.mp4", 8_u8), (0, "a.pdf", 7), (1, "b.pdf", 7)] {
        builder
            .add_with_space_at(
                root,
                Observation {
                    path: vec![name.as_bytes().to_vec()],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact {
                            bytes: 100_u64.into(),
                        },
                    }),
                },
                SpaceObservation {
                    allocated: Some(allocated.clone()),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![file],
                    }),
                    link_count: Some(if file == 7 { 2 } else { 1 }),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(error)?;
    }
    let version = builder
        .seal(Id::from_v7_bytes([2; 16]), ResultCompleteness::Complete)
        .map_err(error)?;
    let whole = |basis| result_totals(&version, &basis, &mut || false).map_err(error);
    assert_eq!(
        whole(SizeBasis::Allocated)?,
        [
            ("type.document", total(2, 4096)),
            ("type.video", total(1, 4096))
        ]
    );
    assert_eq!(
        whole(SizeBasis::Logical)?,
        [
            ("type.document", total(2, 200)),
            ("type.video", total(1, 100))
        ]
    );
    // Adding each root's legend would count the shared file twice.
    let mut documents = 0;
    for root in version.scope_roots() {
        let scope = ItemRef {
            result_version: version.version_id().clone(),
            item_no: root.item_no,
        };
        let totals =
            scope_totals(&version, &scope, &SizeBasis::Allocated, &mut || false).map_err(error)?;
        documents += totals.get("type.document").map_or(0, |t| t.bytes);
    }
    assert_eq!(documents, 8192);
    Ok(())
}
