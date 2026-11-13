use du_analysis_index::query::boundary::{Comparison, NormalizeError, Unit, normalize_size};
use du_analysis_index::query::{self, Basis, Boundary, Condition, Kind, Predicate, Registry, Sort};
use du_analysis_index::{BuildError, IngestSink, Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::Id;
use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};
use du_core_contracts::{ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn error(e: BuildError) -> String {
    format!("{e:?}")
}

#[test]
fn supported_units_map_to_exact_whole_byte_boundaries() -> Result<(), String> {
    let units = [
        (Unit::Byte, 1),
        (Unit::Kilobyte, 1_000),
        (Unit::Megabyte, 1_000_000),
        (Unit::Gigabyte, 1_000_000_000),
        (Unit::Terabyte, 1_000_000_000_000),
        (Unit::Petabyte, 1_000_000_000_000_000),
        (Unit::Kibibyte, 1 << 10),
        (Unit::Mebibyte, 1 << 20),
        (Unit::Gibibyte, 1 << 30),
        (Unit::Tebibyte, 1 << 40),
        (Unit::Pebibyte, 1 << 50),
    ];
    for (unit, bytes) in units {
        let actual = normalize_size(Basis::Logical, Comparison::AtLeast, 1, unit)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            actual,
            Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::AtLeast(bytes)))
        );
        assert_eq!(
            normalize_size(Basis::Allocated, Comparison::LessThan, 0, unit),
            Ok(Condition::Predicate(Predicate::Size(
                Basis::Allocated,
                Boundary::LessThan(0)
            )))
        );
    }
    Ok(())
}

#[test]
fn byte_limits_are_checked_without_rounding_or_saturation() {
    assert_eq!(
        normalize_size(Basis::Logical, Comparison::AtMost, u64::MAX, Unit::Byte),
        Ok(Condition::Predicate(Predicate::Size(
            Basis::Logical,
            Boundary::AtMost(u64::MAX)
        )))
    );
    let largest = u64::MAX / 1024;
    assert_eq!(
        normalize_size(
            Basis::Allocated,
            Comparison::GreaterThan,
            largest,
            Unit::Kibibyte
        ),
        Ok(Condition::Predicate(Predicate::Size(
            Basis::Allocated,
            Boundary::GreaterThan(largest * 1024)
        )))
    );
    assert_eq!(
        normalize_size(
            Basis::Allocated,
            Comparison::GreaterThan,
            largest + 1,
            Unit::Kibibyte
        ),
        Err(NormalizeError::Overflow)
    );
    assert_eq!(
        normalize_size(
            Basis::Logical,
            Comparison::AtLeast,
            u64::MAX,
            Unit::Petabyte
        ),
        Err(NormalizeError::Overflow)
    );
}

#[test]
fn normalized_boundaries_work_in_the_pinned_result_evaluator()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(error)?;
    build.add_directory(vec![]).map_err(error)?;
    for n in [999_u64, 1000, 1001] {
        let value = |basis, n| SizeValue {
            basis,
            evidence: SizeEvidence::OsReported,
            amount: SizeAmount::Exact {
                bytes: ByteSize::from(n),
            },
        };
        build
            .add_with_space(
                Observation {
                    path: vec![n.to_string().into_bytes()],
                    size: Some(value(SizeBasis::Logical, n)),
                },
                SpaceObservation {
                    allocated: Some(value(SizeBasis::Allocated, n)),
                    identity: None,
                    link_count: None,
                    sharing: SharingEvidence::Unknown,
                },
            )
            .map_err(error)?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    for (comparison, expected) in [
        (Comparison::AtLeast, vec!["1000", "1001"]),
        (Comparison::GreaterThan, vec!["1001"]),
        (Comparison::AtMost, vec!["1000", "999"]),
        (Comparison::LessThan, vec!["999"]),
    ] {
        for basis in [Basis::Logical, Basis::Allocated] {
            let condition = Condition::All(vec![
                Condition::Predicate(Predicate::Kind(Kind::File)),
                normalize_size(basis, comparison, 1, Unit::Kilobyte)
                    .map_err(|e| format!("{e:?}"))?,
            ]);
            let result = query::run(
                &Registry::baseline(),
                version.clone(),
                &condition,
                Sort {
                    id: "sort.path",
                    revision: 1,
                    basis,
                    descending: false,
                },
                || false,
            )
            .map_err(|e| format!("{e:?}"))?;
            let names: Vec<_> = result
                .items
                .iter()
                .map(|item| {
                    String::from_utf8_lossy(&version.items()[item.item_no as usize].name)
                        .into_owned()
                })
                .collect();
            assert_eq!(names, expected);
        }
    }
    Ok(())
}
