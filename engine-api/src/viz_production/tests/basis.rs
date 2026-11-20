//! #149: a folder whose children have no value on the unique physical size
//! basis because sharing evidence was not collected names that basis. The
//! fact is read for render requests only; `viz.availability` reads the
//! logical basis, where every child has a value.
use super::*;
use du_analysis_index::{FileIdentity, SharingEvidence, SpaceObservation};

fn space_size(basis: SizeBasis, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

/// Publishes a scope of identified files with unknown sharing, each with an
/// allocated size or without one.
fn publish_spaced(allocated: bool) -> Result<CurrentAnalysis, String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|error| format!("{error:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|error| format!("{error:?}"))?;
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    for name in *b"ab" {
        let observation = Observation {
            path: vec![vec![name]],
            size: size(10),
        };
        let space = SpaceObservation {
            allocated: allocated.then(|| space_size(SizeBasis::Allocated, 4096)),
            identity: Some(FileIdentity {
                volume: vec![1],
                file: vec![name],
            }),
            link_count: Some(1),
            sharing: SharingEvidence::Unknown,
        };
        scan.add_with_space_at(0, observation, space)
            .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    admit_scan(&sink, None, None, Channel::Gui).map_err(|error| format!("{error:?}"))
}

fn render_facts(
    source: &CurrentAnalysis,
    basis: SizeBasis,
) -> Result<crate::required_data::FactValues, String> {
    let mut request = request(policy::COLOR_CRITERION_NONE, "treemap")?;
    request.size_basis = basis;
    producer(&SI_UNITS)
        .required_data_facts(source, &request)
        .map_err(|error| format!("{error:?}"))?
        .ok_or_else(|| "no facts".to_owned())
}

#[test]
fn uncollected_sharing_with_an_allocated_size_names_the_unique_physical_basis() -> TestResult {
    let source = publish_spaced(true)?;
    let unique = render_facts(&source, SizeBasis::UniquePhysical)?;
    assert_eq!(unique.item_sizes, Evidence::Absent);
    assert!(unique.basis_not_counted);
    // The logical and allocated bases have values, so nothing is missing there.
    for basis in [SizeBasis::Logical, SizeBasis::Allocated] {
        let facts = render_facts(&source, basis.clone())?;
        assert_eq!(facts.item_sizes, Evidence::Known(()), "{basis:?}");
        assert!(!facts.basis_not_counted, "{basis:?}");
    }
    Ok(())
}

#[test]
fn availability_reads_the_logical_basis_and_never_names_a_basis() -> TestResult {
    let source = publish_spaced(true)?;
    let facts = producer(&SI_UNITS)
        .required_data_facts_for_scope(&source, &scope())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(facts.item_sizes, Evidence::Known(()));
    assert!(!facts.basis_not_counted);
    Ok(())
}

#[test]
fn without_an_allocated_size_missing_unique_sizes_stay_no_item_sizes() -> TestResult {
    let source = publish_spaced(false)?;
    for basis in [SizeBasis::UniquePhysical, SizeBasis::Allocated] {
        let facts = render_facts(&source, basis.clone())?;
        assert_eq!(facts.item_sizes, Evidence::Absent, "{basis:?}");
        assert!(!facts.basis_not_counted, "{basis:?}");
    }
    Ok(())
}
