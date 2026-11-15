//! Assemble the size aggregation rules for the desktop analysis flow (F03 FR14.HD2.app-host.DD3).
use std::io;

use du_engine_api::size_rule::{
    SizeRuleRegistryBuilder, SizeRuleSetupError, SizeRuleSource, SizeRules,
    register_default_size_rules,
};

#[cfg(test)]
mod tests;

/// Registers the default size rule bundle once and pairs it with `source`.
///
/// A duplicate or missing registration stops startup with the
/// `startup.registration_failed` convention the other registrations follow.
pub(super) fn assemble(source: SizeRuleSource) -> Result<SizeRules, io::Error> {
    let mut builder = SizeRuleRegistryBuilder::new();
    register_default_size_rules(&mut builder).map_err(registration_failed)?;
    assemble_from(builder, source)
}

/// Freezes an already filled builder; a missing default rule stops startup.
pub(super) fn assemble_from(
    builder: SizeRuleRegistryBuilder,
    source: SizeRuleSource,
) -> Result<SizeRules, io::Error> {
    SizeRules::assemble(builder, source).map_err(registration_failed)
}

fn registration_failed(error: SizeRuleSetupError) -> io::Error {
    io::Error::other(format!("startup.registration_failed: {error}"))
}
