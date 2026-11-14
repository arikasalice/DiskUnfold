//! The pin wrappers apply the exposure table at the moment of the pin.

use du_analysis_index::{IngestSink, SampleSessionKey};
use du_core_contracts::{
    PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};

use super::*;

type Check = Result<(), String>;

fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}

/// A store with one sample version (id 2) and one real scan version (id 4, lineage bound).
fn store() -> Result<IngestSink, String> {
    let store = IngestSink::default();
    let debug = |error: &dyn std::fmt::Debug| format!("{error:?}");
    let mut sample = store
        .open_sample_forest(id(1), vec![root()], &SampleSessionKey::new(id(9)))
        .map_err(|e| debug(&e))?;
    sample.add_directory_at(0, vec![]).map_err(|e| debug(&e))?;
    sample
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| debug(&e))?;
    let lineage = store
        .bind_current_analysis("real", id(5))
        .map_err(|e| debug(&e))?;
    let mut real = store
        .open_forest_in_lineage(id(3), vec![root()], lineage)
        .map_err(|e| debug(&e))?;
    real.add_directory_at(0, vec![]).map_err(|e| debug(&e))?;
    real.seal(id(4), ResultCompleteness::Complete)
        .map_err(|e| debug(&e))?;
    Ok(store)
}

#[test]
fn a_sample_version_is_pinned_only_for_the_gui() -> Check {
    let store = store()?;
    assert!(pin_current_for(&store, &id(2), Channel::Gui).is_ok());
    assert!(pin_scan_for(&store, None, Some(&id(2)), Channel::Gui).is_ok());
    for channel in [Channel::Mcp, Channel::Cli] {
        assert_eq!(
            pin_current_for(&store, &id(2), channel).err(),
            Some(PinError::Hidden)
        );
        assert_eq!(
            pin_scan_for(&store, None, Some(&id(2)), channel).err(),
            Some(PinError::Hidden)
        );
    }
    Ok(())
}

#[test]
fn a_real_version_is_pinned_on_every_channel() -> Check {
    let store = store()?;
    for channel in [Channel::Gui, Channel::Mcp, Channel::Cli] {
        assert!(
            pin_current_for(&store, &id(4), channel).is_ok(),
            "{channel:?}"
        );
        assert!(
            pin_scan_for(&store, None, None, channel).is_ok(),
            "{channel:?}"
        );
        let latest = pin_latest_for(&store, channel).map_err(|e| format!("{e:?}"))?;
        assert!(latest.is_some(), "{channel:?}");
    }
    Ok(())
}

#[test]
fn store_failures_pass_through_and_a_hidden_version_reads_as_missing() -> Check {
    let store = store()?;
    // The store reports an unknown identifier as a superseded version.
    assert_eq!(
        pin_current_for(&store, &id(7), Channel::Mcp).err(),
        Some(PinError::Query(QueryError::StaleVersion))
    );
    assert_eq!(PinError::Hidden.query(), QueryError::NotFound);
    assert_eq!(
        PinError::Query(QueryError::StaleVersion).query(),
        QueryError::StaleVersion
    );
    // A sample version is never the implicit latest result, whatever the channel.
    let sample_only = IngestSink::default();
    let mut build = sample_only
        .open_sample_forest(id(1), vec![root()], &SampleSessionKey::new(id(9)))
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    for channel in [Channel::Gui, Channel::Mcp, Channel::Cli] {
        let latest = pin_latest_for(&sample_only, channel).map_err(|e| format!("{e:?}"))?;
        assert!(latest.is_none(), "{channel:?}");
    }
    Ok(())
}
