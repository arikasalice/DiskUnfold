//! #217: excluded direct children counted from a folder's single ended attempt.
use super::*;
use crate::folder_collection::ExcludedChildren;
use du_core_contracts::{ItemRef, scan_observation::FilterDecision};

impl Attempt {
    /// Excludes one direct child; the callback is its only terminal.
    fn exclude(&mut self, build: &mut BuildHandle, name: &str) -> Test {
        let child_id = self.children;
        self.children += 1;
        self.send(
            build,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name: name.as_bytes().to_vec(),
                decision: FilterDecision::Exclude,
                phase: FilterCallbackPhase::BeforeMetadata,
            }),
        )
    }
}

/// The root folder with the file `a`, read by `attempts` attempts that each exclude
/// `excluded` and end when `end` is set, in a version sealed `Missing`.
fn excluded_at_root(
    excluded: &[&str],
    attempts: u64,
    end: bool,
) -> Result<Option<ExcludedChildren>, String> {
    let sink = IngestSink::default();
    let mut build = sink.open_forest(id(1)?, vec![root()]).map_err(e)?;
    build.add_directory(Vec::new()).map_err(e)?;
    build
        .add_file(Observation {
            path: path(&["a"]),
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: exact(1),
            }),
        })
        .map_err(e)?;
    for number in 0..attempts {
        let mut attempt = Attempt::begin(&mut build, &[], 20 + number)?;
        attempt.file(&mut build, "a")?;
        for name in excluded {
            attempt.exclude(&mut build, name)?;
        }
        if end {
            attempt.end(&mut build)?;
        }
    }
    let version = build.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    version
        .folder_excluded_children(&ItemRef {
            result_version: version.version_id.clone(),
            item_no: 0,
        })
        .map_err(e)
}

#[test]
fn a_single_ended_attempt_gives_the_exact_excluded_count() -> Test {
    assert_eq!(
        excluded_at_root(&["x", "y"], 1, true)?,
        Some(ExcludedChildren {
            count: 2,
            exact: true
        })
    );
    // Without exclusion rules every child is admitted: exactly none was excluded.
    assert_eq!(
        excluded_at_root(&[], 1, true)?,
        Some(ExcludedChildren {
            count: 0,
            exact: true
        })
    );
    Ok(())
}

#[test]
fn no_count_without_a_single_ended_attempt() -> Test {
    // The attempt did not end.
    assert_eq!(excluded_at_root(&["x"], 1, false)?, None);
    // Two attempts listed the folder; neither is chosen.
    assert_eq!(excluded_at_root(&["x"], 2, true)?, None);
    Ok(())
}
