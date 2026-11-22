//! A model built from a sample version is refused for the channels that must not see it.
use super::*;
use crate::api::Channel;
use du_analysis_index::SampleSessionKey;
use du_core_contracts::{
    PathBytes, PathEncoding,
    result::{ResultNativePath, ResultScopeRoot},
};

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

#[test]
fn a_sample_model_is_refused_for_external_channels_before_any_read() -> Result<(), String> {
    let source = IngestSink::default();
    let mut sample = source
        .open_sample_forest(id(1)?, vec![root()], &SampleSessionKey::new(id(9)?))
        .map_err(|e| format!("{e:?}"))?;
    sample
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    let hidden = sample
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut real = source.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    real.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let shown = real
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = ModelPages::new(
        source,
        NonZeroUsize::new(2).ok_or("capacity")?,
        Arc::new(Ids(AtomicU64::new(1))),
    );
    let register = |version: &Arc<ResultVersion>, fingerprint: &str| {
        pages
            .register(ModelInput {
                version: version.clone(),
                size_basis: du_core_contracts::SizeBasis::Logical,
                condition_fingerprint: fingerprint.into(),
                rows: vec![],
                groups: HashMap::new(),
            })
            .map_err(|e| format!("{e:?}"))
    };
    let sample_model = register(&hidden, "sample")?;
    let real_model = register(&shown, "real")?;
    for channel in [Channel::Mcp, Channel::Cli] {
        assert_eq!(
            pages.refuse_hidden(hidden.version_id(), channel),
            Err(Error::ItemUnavailable)
        );
        assert_eq!(
            pages.refuse_hidden_model(&sample_model, channel),
            Err(Error::ItemUnavailable)
        );
    }
    for channel in [Channel::Gui, Channel::Mcp, Channel::Cli] {
        assert_eq!(pages.refuse_hidden(shown.version_id(), channel), Ok(()));
        assert_eq!(pages.refuse_hidden_model(&real_model, channel), Ok(()));
    }
    assert_eq!(
        pages.refuse_hidden(hidden.version_id(), Channel::Gui),
        Ok(())
    );
    assert_eq!(
        pages.refuse_hidden_model(&sample_model, Channel::Gui),
        Ok(())
    );
    // An unknown model or revision passes, so the read that follows reports its usual error.
    assert_eq!(pages.refuse_hidden_model("missing", Channel::Mcp), Ok(()));
    assert_eq!(pages.refuse_hidden(&id(77)?, Channel::Mcp), Ok(()));
    Ok(())
}
