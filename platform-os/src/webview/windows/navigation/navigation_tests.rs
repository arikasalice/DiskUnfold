use super::{NavigationCompletion, NavigationIdFence};

#[test]
fn only_the_current_allowed_navigation_can_ready_or_fail_the_document() {
    let current = NavigationIdFence::default();
    current.set_current(10);
    current.set_current(11);

    assert_eq!(
        current.classify_completion(10, true),
        NavigationCompletion::Stale
    );
    assert_eq!(
        current.classify_completion(10, false),
        NavigationCompletion::Stale
    );
    assert_eq!(
        current.classify_completion(11, true),
        NavigationCompletion::Succeeded
    );
    assert_eq!(
        current.classify_completion(11, false),
        NavigationCompletion::Failed
    );
}
