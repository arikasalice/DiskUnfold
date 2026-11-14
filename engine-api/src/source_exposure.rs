//! Source exposure table for external channels (FR2.HD4.app-host.DD1, IF-18).
//!
//! The GUI shows sample results labelled as samples. CLI and MCP callers must
//! not learn that a sample result exists, so a pinned sample version is
//! reported as `ref.unavailable` on those channels.

use du_core_contracts::{data_source::DataSource, result::ResultVersionRead};

use crate::{
    api::{CallerCtx, Channel},
    error::EngineError,
};

/// Whether a result of some source may be served on a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exposure {
    Allow,
    Hide,
}

/// Returns the exposure of a result of `source` on `channel`.
pub(crate) fn exposure(source: DataSource, channel: Channel) -> Exposure {
    match (channel, source) {
        (Channel::Gui, _) | (Channel::Mcp | Channel::Cli, DataSource::Real) => Exposure::Allow,
        (Channel::Mcp | Channel::Cli, DataSource::Sample) => Exposure::Hide,
    }
}

/// Checks a result source against the caller's channel right after it is pinned.
///
/// A hidden source fails with `ref.unavailable`, the same reason as a missing
/// reference, before any content of the version is read.
pub(crate) fn admit_source(source: DataSource, caller: &CallerCtx) -> Result<(), EngineError> {
    match exposure(source, caller.channel) {
        Exposure::Allow => Ok(()),
        Exposure::Hide => Err(EngineError::reason("ref.unavailable")),
    }
}

/// Checks a pinned result version against the caller's channel.
///
/// Call it directly after pinning, before the version is projected or paged.
pub(crate) fn admit_version_source(
    version: &dyn ResultVersionRead,
    caller: &CallerCtx,
) -> Result<(), EngineError> {
    admit_source(version.data_source(), caller)
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{
        ErrorCode, Id,
        result::{ResultCompleteness, ResultItem},
    };

    struct Version(Id, DataSource);

    impl ResultVersionRead for Version {
        fn version_id(&self) -> &Id {
            &self.0
        }
        fn scope_version_id(&self) -> &Id {
            &self.0
        }
        fn completeness(&self) -> ResultCompleteness {
            ResultCompleteness::Complete
        }
        fn items(&self) -> &[ResultItem] {
            &[]
        }
        fn data_source(&self) -> DataSource {
            self.1
        }
    }

    fn caller(channel: Channel) -> Result<CallerCtx, String> {
        let id = Id::parse("018f0000-0000-7000-8000-000000000001").map_err(|e| e.to_string())?;
        Ok(CallerCtx::new(channel, id, None))
    }

    #[test]
    fn exposure_table_hides_samples_only_from_external_channels() {
        use Channel::{Cli, Gui, Mcp};
        use DataSource::{Real, Sample};
        use Exposure::{Allow, Hide};
        let rows = [
            (Real, Gui, Allow),
            (Sample, Gui, Allow),
            (Real, Mcp, Allow),
            (Sample, Mcp, Hide),
            (Real, Cli, Allow),
            (Sample, Cli, Hide),
        ];
        for (source, channel, expected) in rows {
            assert_eq!(
                exposure(source, channel),
                expected,
                "{source:?} {channel:?}"
            );
        }
    }

    #[test]
    fn hidden_sources_fail_as_unavailable_references() -> Result<(), String> {
        use Channel::{Cli, Gui, Mcp};
        use DataSource::{Real, Sample};
        let rows = [
            (Real, Gui, true),
            (Sample, Gui, true),
            (Real, Mcp, true),
            (Sample, Mcp, false),
            (Real, Cli, true),
            (Sample, Cli, false),
        ];
        for (source, channel, allowed) in rows {
            let result = admit_source(source, &caller(channel)?);
            if allowed {
                assert_eq!(result, Ok(()), "{source:?} {channel:?}");
            } else {
                assert_eq!(
                    result,
                    Err(EngineError::reason("ref.unavailable")),
                    "{source:?} {channel:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn version_check_reads_the_source_recorded_on_the_version() -> Result<(), String> {
        let id = Id::parse("018f0000-0000-7000-8000-000000000002").map_err(|e| e.to_string())?;
        let rows = [
            (DataSource::Sample, Channel::Mcp, false),
            (DataSource::Sample, Channel::Cli, false),
            (DataSource::Sample, Channel::Gui, true),
            (DataSource::Real, Channel::Mcp, true),
        ];
        for (source, channel, allowed) in rows {
            let version = Version(id.clone(), source);
            let result = admit_version_source(&version, &caller(channel)?);
            if allowed {
                assert_eq!(result, Ok(()), "{source:?} {channel:?}");
                continue;
            }
            let envelope = result
                .err()
                .ok_or("expected a rejection")?
                .into_envelope(id.clone());
            assert_eq!(envelope.reason_key, "ref.unavailable");
            assert_eq!(envelope.code, ErrorCode::NotFound);
        }
        Ok(())
    }
}
