use super::super::*;
use super::{Arc, Mutex, VecDeque};

#[test]
fn a_post_failure_reports_the_failed_event_and_discards_the_rest_of_its_batch()
-> Result<(), Box<dyn std::error::Error>> {
    let pending = Arc::new(Mutex::new(VecDeque::from([
        "first".to_owned(),
        "second".to_owned(),
        "third".to_owned(),
    ])));
    let source = pending.clone();
    let mut posted = Vec::new();
    let mut failures = Vec::new();
    let mut fail_first = true;
    flush_event_batch(
        move || source.lock().ok()?.pop_front(),
        |wire| {
            posted.push(wire.to_owned());
            if std::mem::take(&mut fail_first) {
                Err("injected native post failure")
            } else {
                Ok(())
            }
        },
        |failure| failures.push(failure),
    );

    assert_eq!(posted, ["event\nfirst"]);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].failed, "first");
    assert_eq!(failures[0].discarded, ["second", "third"]);
    assert!(pending.lock().map_err(|_| "event source lock")?.is_empty());
    Ok(())
}

#[test]
fn a_middle_post_failure_keeps_successful_prefix_out_of_the_discard_report()
-> Result<(), Box<dyn std::error::Error>> {
    let pending = Arc::new(Mutex::new(VecDeque::from([
        "accepted".to_owned(),
        "failed".to_owned(),
        "discarded-a".to_owned(),
        "discarded-b".to_owned(),
    ])));
    let source = pending.clone();
    let mut posted = Vec::new();
    let mut failures = Vec::new();
    let mut fail_next = true;
    flush_event_batch(
        move || source.lock().ok()?.pop_front(),
        |wire| {
            posted.push(wire.to_owned());
            if wire == "event\nfailed" && std::mem::take(&mut fail_next) {
                Err("injected middle failure")
            } else {
                Ok(())
            }
        },
        |failure| failures.push(failure),
    );

    assert_eq!(posted, ["event\naccepted", "event\nfailed"]);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].failed, "failed");
    assert_eq!(failures[0].discarded, ["discarded-a", "discarded-b"]);
    assert!(pending.lock().map_err(|_| "event source lock")?.is_empty());
    Ok(())
}

#[test]
fn failure_on_the_last_event_has_no_recovery_before_the_next_flush()
-> Result<(), Box<dyn std::error::Error>> {
    let pending = Arc::new(Mutex::new(VecDeque::from([
        "first".to_owned(),
        "last".to_owned(),
    ])));
    let source = pending.clone();
    let mut posted = Vec::new();
    let mut failures = Vec::new();
    let mut failed_last = false;
    let mut native_post = |wire: &str| {
        posted.push(wire.to_owned());
        if wire == "event\nlast" && !failed_last {
            failed_last = true;
            Err("injected final post failure")
        } else {
            Ok(())
        }
    };

    flush_event_batch(
        move || source.lock().ok()?.pop_front(),
        &mut native_post,
        |failure| failures.push(failure),
    );
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].failed, "last");
    assert!(failures[0].discarded.is_empty());
    pending
        .lock()
        .map_err(|_| "event source lock")?
        .push_back("next-flush".to_owned());

    let source = pending.clone();
    flush_event_batch(
        move || source.lock().ok()?.pop_front(),
        &mut native_post,
        |failure| failures.push(failure),
    );
    drop(native_post);
    assert_eq!(posted, ["event\nfirst", "event\nlast", "event\nnext-flush"]);
    assert_eq!(failures.len(), 1);
    Ok(())
}

#[test]
fn separate_event_sources_do_not_share_post_failure_state_and_stop_when_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let first_queue = Arc::new(Mutex::new(VecDeque::from(["first-window".to_owned()])));
    let first_source = first_queue.clone();
    let mut first_failures = Vec::new();
    flush_event_batch(
        move || first_source.lock().ok()?.pop_front(),
        |_| Err("first window failure"),
        |failure| first_failures.push(failure),
    );

    let second_queue = Arc::new(Mutex::new(VecDeque::from([
        "second-window-a".to_owned(),
        "second-window-b".to_owned(),
    ])));
    let second_source = second_queue.clone();
    let mut second_posts = Vec::new();
    let mut second_failures = Vec::new();
    flush_event_batch(
        move || second_source.lock().ok()?.pop_front(),
        |wire| {
            second_posts.push(wire.to_owned());
            Ok::<(), &'static str>(())
        },
        |failure| second_failures.push(failure),
    );

    assert_eq!(first_queue.lock().map_err(|_| "first queue lock")?.len(), 0);
    assert_eq!(
        second_queue.lock().map_err(|_| "second queue lock")?.len(),
        0
    );
    assert_eq!(first_failures.len(), 1);
    assert_eq!(first_failures[0].failed, "first-window");
    assert!(first_failures[0].discarded.is_empty());
    assert!(second_failures.is_empty());
    assert_eq!(
        second_posts,
        ["event\nsecond-window-a", "event\nsecond-window-b"]
    );

    let mut closed = true;
    let mut closed_posts = 0;
    flush_event_batch(
        || (!std::mem::take(&mut closed)).then(|| "after-close".to_owned()),
        |_| {
            closed_posts += 1;
            Ok::<(), &'static str>(())
        },
        |_| {},
    );
    assert_eq!(closed_posts, 0);
    Ok(())
}
