//! The product visualization registration of the desktop assembly (FR10.HD1.app-host.DD2).
use std::{num::NonZeroUsize, time::Duration};

use du_engine_api::{
    api::EngineBuilder,
    desktop::{DefaultVisualization, VisualizationServices},
    viz_production::UnitTableSource,
    viz_runtime::ViewPolicy,
};

#[cfg(test)]
mod tests;

// Provisional until measured (F06 DLD Q4). The view policy and the model cache size are
// built-in values for now; a later change reads them from the display settings.
const POLICY_REVISION: u32 = 1;
const MAX_SCALE: f64 = 4.0;
/// The GUI draws one DOM element and one button per shape, so the cap starts low.
const MAX_SHAPES: usize = 2000;
// A count of n is written NonZeroUsize::MIN.saturating_add(n - 1), which cannot be zero.
/// 8 open view channels.
const MAX_CHANNELS: NonZeroUsize = NonZeroUsize::MIN.saturating_add(7);
/// 8 concurrent computations.
const MAX_WORK: NonZeroUsize = NonZeroUsize::MIN.saturating_add(7);
/// Matches the wait the folder browse commands use before they return a job.
const SYNCHRONOUS_WAIT: Duration = Duration::from_millis(300);
/// 16 retained models.
const MODEL_CAPACITY: NonZeroUsize = NonZeroUsize::MIN.saturating_add(15);

fn options(unit_table: UnitTableSource) -> DefaultVisualization {
    DefaultVisualization {
        policy: ViewPolicy {
            revision: POLICY_REVISION,
            max_scale: MAX_SCALE,
            max_shapes: MAX_SHAPES,
            max_channels: MAX_CHANNELS,
            max_work: MAX_WORK,
            synchronous_wait: SYNCHRONOUS_WAIT,
        },
        model_capacity: MODEL_CAPACITY,
        unit_table,
    }
}

/// Registers the product visualization once on this desktop's engine.
///
/// A duplicate registration, a missing strategy or a rejected policy returns the
/// engine's reason, which stops startup.
pub(super) fn register(
    services: &VisualizationServices,
    builder: &mut EngineBuilder,
    unit_table: UnitTableSource,
) -> Result<(), &'static str> {
    services.register_default(
        builder,
        &du_platform_os::entropy::OsRandom,
        options(unit_table),
    )
}

/// Registers the visualization for a test engine whose analysis flow was composed alone.
#[cfg(test)]
pub(super) fn register_with_stored_settings(
    services: &VisualizationServices,
    builder: &mut EngineBuilder,
    database: &crate::shared_database::SharedDatabase,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings = std::sync::Arc::new(crate::settings::StartupSettings::open(
        database,
        1,
        &["en", "ko"],
    )?);
    register(services, builder, settings.unit_table_source())?;
    Ok(())
}
