use std::cell::Cell;

#[derive(Default)]
pub(super) struct NavigationIdFence(Cell<Option<u64>>);

impl NavigationIdFence {
    pub(super) fn set_current(&self, navigation_id: u64) {
        self.0.set(Some(navigation_id));
    }

    pub(super) fn classify_completion(
        &self,
        navigation_id: u64,
        success: bool,
    ) -> NavigationCompletion {
        classify_navigation_completion(self.0.get(), navigation_id, success)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavigationCompletion {
    Stale,
    Succeeded,
    Failed,
}

fn classify_navigation_completion(
    current_navigation: Option<u64>,
    completed_navigation: u64,
    success: bool,
) -> NavigationCompletion {
    if current_navigation != Some(completed_navigation) {
        NavigationCompletion::Stale
    } else if success {
        NavigationCompletion::Succeeded
    } else {
        NavigationCompletion::Failed
    }
}

#[cfg(test)]
mod navigation_tests;
