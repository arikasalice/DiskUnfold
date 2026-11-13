//! Registered bar implementations over ranked or classified evidence.

use super::{BarError, BarInput, Strategy};
use crate::visualization::{bar_categories, bar_projection};

/// Ranked bar implementation used by the explicit preview registry.
pub struct RankedBar;

impl Strategy for RankedBar {
    fn bar(
        &self,
        input: BarInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<bar_projection::Ranking, BarError> {
        match input {
            BarInput::Ranked(request) => {
                bar_projection::project_children(request, cancelled).map_err(BarError::Projection)
            }
            BarInput::PreparedRanked(request) => {
                request.render(cancelled).map_err(BarError::Projection)
            }
            BarInput::TypeComparison | BarInput::Stacked => Err(BarError::UnsupportedMode),
        }
    }
}

/// One strategy identity supporting all three published bar representations.
pub(in crate::visualization) struct BarLayout;

impl Strategy for BarLayout {
    fn bar(
        &self,
        input: BarInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<bar_projection::Ranking, BarError> {
        RankedBar.bar(input, cancelled)
    }

    fn category_bar(
        &self,
        input: bar_categories::Input<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<bar_categories::Layout, BarError> {
        bar_categories::calculate(input, cancelled).map_err(BarError::Category)
    }

    fn complete_bar(&self) -> bool {
        true
    }
}
